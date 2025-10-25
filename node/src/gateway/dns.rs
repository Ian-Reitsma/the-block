use super::{mobile_cache, read_receipt};
use crate::governance::NODE_GOV_STORE;
use crate::simple_db::{names, SimpleDb};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::ERR_DNS_SIG_INVALID;
use concurrency::Lazy;
use crypto_suite::signatures::ed25519::{
    Signature, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH,
};
#[cfg(feature = "telemetry")]
use diagnostics::tracing::warn;
use foundation_serialization::binary_cursor::{Reader as BinaryReader, Writer as BinaryWriter};
use foundation_serialization::json::{Map, Number, Value};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "telemetry")]
use crate::telemetry::{DNS_VERIFICATION_FAIL_TOTAL, GATEWAY_DNS_LOOKUP_TOTAL};
use runtime::net::lookup_txt;

static DNS_DB: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_DNS_DB_PATH").unwrap_or_else(|_| "dns_db".into());
    Mutex::new(SimpleDb::open_named(names::GATEWAY_DNS, &path))
});

static ALLOW_EXTERNAL: AtomicBool = AtomicBool::new(false);
static DISABLE_VERIFY: AtomicBool = AtomicBool::new(false);
const VERIFY_TTL: Duration = Duration::from_secs(3600);

type TxtResolver = Box<dyn Fn(&str) -> Vec<String> + Send + Sync>;
static TXT_RESOLVER: Lazy<Mutex<TxtResolver>> =
    Lazy::new(|| Mutex::new(Box::new(default_txt_resolver)));
static VERIFY_CACHE: Lazy<Mutex<HashMap<String, (bool, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static TREASURY_HOOK: Lazy<Mutex<Option<Arc<dyn Fn(u64) + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
enum AuctionStatus {
    Active,
    Settled,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainBidRecord {
    bidder: String,
    amount_ct: u64,
    stake_reference: Option<String>,
    placed_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainAuctionRecord {
    domain: String,
    seller_account: Option<String>,
    seller_stake: Option<String>,
    protocol_fee_bps: u16,
    royalty_bps: u16,
    min_bid_ct: u64,
    stake_requirement_ct: u64,
    start_ts: u64,
    end_ts: u64,
    status: AuctionStatus,
    highest_bid: Option<DomainBidRecord>,
    bids: Vec<DomainBidRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainOwnershipRecord {
    domain: String,
    owner_account: String,
    acquired_at: u64,
    royalty_bps: u16,
    last_sale_price_ct: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainSaleRecord {
    domain: String,
    sold_at: u64,
    seller_account: Option<String>,
    buyer_account: String,
    price_ct: u64,
    protocol_fee_ct: u64,
    royalty_fee_ct: u64,
}

#[derive(Debug)]
pub enum AuctionError {
    InvalidDomain,
    VerificationRequired,
    AlreadyListed,
    ListingActive,
    OwnershipMismatch,
    AuctionMissing,
    AuctionClosed,
    AuctionExpired,
    AuctionNotFinished,
    BidTooLow,
    BidInsufficientStake,
    InvalidBidder,
    NoBids,
    Storage,
}

impl AuctionError {
    pub fn code(&self) -> i32 {
        match self {
            AuctionError::InvalidDomain => -32060,
            AuctionError::VerificationRequired => -32061,
            AuctionError::AlreadyListed => -32062,
            AuctionError::ListingActive => -32063,
            AuctionError::OwnershipMismatch => -32064,
            AuctionError::AuctionMissing => -32065,
            AuctionError::AuctionClosed => -32066,
            AuctionError::AuctionExpired => -32067,
            AuctionError::AuctionNotFinished => -32068,
            AuctionError::BidTooLow => -32069,
            AuctionError::BidInsufficientStake => -32070,
            AuctionError::InvalidBidder => -32071,
            AuctionError::NoBids => -32072,
            AuctionError::Storage => -32073,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            AuctionError::InvalidDomain => "invalid domain for auction",
            AuctionError::VerificationRequired => "domain verification required",
            AuctionError::AlreadyListed => "domain already listed",
            AuctionError::ListingActive => "domain auction already active",
            AuctionError::OwnershipMismatch => "seller does not own domain",
            AuctionError::AuctionMissing => "domain auction not found",
            AuctionError::AuctionClosed => "auction closed",
            AuctionError::AuctionExpired => "auction expired",
            AuctionError::AuctionNotFinished => "auction still running",
            AuctionError::BidTooLow => "bid below current minimum",
            AuctionError::BidInsufficientStake => "bid does not satisfy stake requirement",
            AuctionError::InvalidBidder => "invalid bidder account",
            AuctionError::NoBids => "auction has no bids",
            AuctionError::Storage => "auction storage error",
        }
    }
}

fn auction_key(domain: &str) -> String {
    format!("dns_auction/{domain}")
}

fn ownership_key(domain: &str) -> String {
    format!("dns_ownership/{domain}")
}

fn sale_history_key(domain: &str) -> String {
    format!("dns_sales/{domain}")
}

fn decode_auction(bytes: &[u8]) -> Result<DomainAuctionRecord, AuctionError> {
    let mut reader = BinaryReader::new(bytes);
    let record = read_auction(&mut reader).map_err(map_decode_error)?;
    ensure_exhausted(&reader).map_err(map_decode_error)?;
    Ok(record)
}

fn encode_auction(record: &DomainAuctionRecord) -> Result<Vec<u8>, AuctionError> {
    let mut writer = BinaryWriter::new();
    write_auction(&mut writer, record);
    Ok(writer.finish())
}

fn decode_ownership(bytes: &[u8]) -> Result<DomainOwnershipRecord, AuctionError> {
    let mut reader = BinaryReader::new(bytes);
    let record = read_ownership(&mut reader).map_err(map_decode_error)?;
    ensure_exhausted(&reader).map_err(map_decode_error)?;
    Ok(record)
}

fn encode_ownership(record: &DomainOwnershipRecord) -> Result<Vec<u8>, AuctionError> {
    let mut writer = BinaryWriter::new();
    write_ownership(&mut writer, record);
    Ok(writer.finish())
}

fn decode_sales(bytes: &[u8]) -> Result<Vec<DomainSaleRecord>, AuctionError> {
    let mut reader = BinaryReader::new(bytes);
    let values = reader
        .read_vec_with(|r| read_sale(r))
        .map_err(map_decode_error)?;
    ensure_exhausted(&reader).map_err(map_decode_error)?;
    Ok(values)
}

fn encode_sales(records: &[DomainSaleRecord]) -> Result<Vec<u8>, AuctionError> {
    let mut writer = BinaryWriter::new();
    writer.write_vec_with(records, write_sale);
    Ok(writer.finish())
}

fn record_treasury_fee(amount_ct: u64) {
    if amount_ct == 0 {
        return;
    }
    if let Some(hook) = TREASURY_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .cloned()
    {
        hook(amount_ct);
        return;
    }
    if let Err(err) = NODE_GOV_STORE.record_treasury_accrual(amount_ct) {
        #[cfg(feature = "telemetry")]
        warn!(
            amount_ct,
            ?err,
            "failed to accrue treasury fee from dns auction"
        );
        #[cfg(not(feature = "telemetry"))]
        let _ = (amount_ct, err);
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
pub fn install_treasury_hook<F>(hook: F)
where
    F: Fn(u64) + Send + Sync + 'static,
{
    *TREASURY_HOOK.lock().unwrap() = Some(Arc::new(hook));
}

#[cfg(test)]
pub fn clear_treasury_hook() {
    TREASURY_HOOK.lock().unwrap().take();
}

fn status_label(status: AuctionStatus) -> &'static str {
    match status {
        AuctionStatus::Active => "active",
        AuctionStatus::Settled => "settled",
        AuctionStatus::Cancelled => "cancelled",
    }
}

fn bid_to_json(bid: &DomainBidRecord) -> Value {
    json_map(vec![
        ("bidder", Value::String(bid.bidder.clone())),
        ("amount_ct", Value::Number(Number::from(bid.amount_ct))),
        ("placed_at", Value::Number(Number::from(bid.placed_at))),
        (
            "stake_reference",
            bid.stake_reference
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
    ])
}

fn auction_to_json(record: &DomainAuctionRecord) -> Value {
    let bids = Value::Array(record.bids.iter().map(bid_to_json).collect());
    let highest = record
        .highest_bid
        .as_ref()
        .map(bid_to_json)
        .unwrap_or(Value::Null);
    json_map(vec![
        ("domain", Value::String(record.domain.clone())),
        (
            "seller_account",
            record
                .seller_account
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
        (
            "seller_stake",
            record
                .seller_stake
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
        (
            "protocol_fee_bps",
            Value::Number(Number::from(record.protocol_fee_bps)),
        ),
        (
            "royalty_bps",
            Value::Number(Number::from(record.royalty_bps)),
        ),
        ("min_bid_ct", Value::Number(Number::from(record.min_bid_ct))),
        (
            "stake_requirement_ct",
            Value::Number(Number::from(record.stake_requirement_ct)),
        ),
        ("start_ts", Value::Number(Number::from(record.start_ts))),
        ("end_ts", Value::Number(Number::from(record.end_ts))),
        (
            "status",
            Value::String(status_label(record.status).to_string()),
        ),
        ("highest_bid", highest),
        ("bids", bids),
    ])
}

fn ownership_to_json(record: &DomainOwnershipRecord) -> Value {
    json_map(vec![
        ("domain", Value::String(record.domain.clone())),
        ("owner_account", Value::String(record.owner_account.clone())),
        (
            "acquired_at",
            Value::Number(Number::from(record.acquired_at)),
        ),
        (
            "royalty_bps",
            Value::Number(Number::from(record.royalty_bps)),
        ),
        (
            "last_sale_price_ct",
            Value::Number(Number::from(record.last_sale_price_ct)),
        ),
    ])
}

fn sale_to_json(record: &DomainSaleRecord) -> Value {
    json_map(vec![
        ("domain", Value::String(record.domain.clone())),
        (
            "seller_account",
            record
                .seller_account
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
        ("buyer_account", Value::String(record.buyer_account.clone())),
        ("sold_at", Value::Number(Number::from(record.sold_at))),
        ("price_ct", Value::Number(Number::from(record.price_ct))),
        (
            "protocol_fee_ct",
            Value::Number(Number::from(record.protocol_fee_ct)),
        ),
        (
            "royalty_fee_ct",
            Value::Number(Number::from(record.royalty_fee_ct)),
        ),
    ])
}

fn ensure_domain_allowed(domain: &str, db: &SimpleDb) -> Result<(), AuctionError> {
    if domain.is_empty() {
        return Err(AuctionError::InvalidDomain);
    }
    if domain.ends_with(".block") {
        return Ok(());
    }
    let key = format!("dns_keys/{domain}");
    if let Some(bytes) = db.get(&key) {
        if let Ok(pk) = String::from_utf8(bytes) {
            if verify_txt(domain, &pk) {
                return Ok(());
            }
        }
    }
    Err(AuctionError::VerificationRequired)
}

const BID_FIELD_COUNT: u64 = 4;
const AUCTION_FIELD_COUNT: u64 = 12;
const OWNERSHIP_FIELD_COUNT: u64 = 5;
const SALE_FIELD_COUNT: u64 = 7;

fn write_bid(writer: &mut BinaryWriter, bid: &DomainBidRecord) {
    writer.write_struct(|s| {
        s.field_string("bidder", &bid.bidder);
        s.field_u64("amount_ct", bid.amount_ct);
        s.field_option_string("stake_reference", bid.stake_reference.as_deref());
        s.field_u64("placed_at", bid.placed_at);
    });
}

fn read_bid(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainBidRecord> {
    let mut bidder = None;
    let mut amount = None;
    let mut stake: Option<Option<String>> = None;
    let mut placed_at = None;

    decode_struct(reader, Some(BID_FIELD_COUNT), |key, reader| match key {
        "bidder" => {
            let value = reader.read_string()?;
            assign_once(&mut bidder, value, "bidder")
        }
        "amount_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount_ct")
        }
        "stake_reference" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut stake, value, "stake_reference")
        }
        "placed_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut placed_at, value, "placed_at")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainBidRecord {
        bidder: bidder.ok_or(DecodeError::MissingField("bidder"))?,
        amount_ct: amount.ok_or(DecodeError::MissingField("amount_ct"))?,
        stake_reference: stake.unwrap_or(None),
        placed_at: placed_at.ok_or(DecodeError::MissingField("placed_at"))?,
    })
}

fn status_to_u8(status: AuctionStatus) -> u8 {
    match status {
        AuctionStatus::Active => 0,
        AuctionStatus::Settled => 1,
        AuctionStatus::Cancelled => 2,
    }
}

fn status_from_u8(value: u8) -> binary_struct::Result<AuctionStatus> {
    match value {
        0 => Ok(AuctionStatus::Active),
        1 => Ok(AuctionStatus::Settled),
        2 => Ok(AuctionStatus::Cancelled),
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "AuctionStatus",
            value: other as u32,
        }),
    }
}

fn write_auction(writer: &mut BinaryWriter, record: &DomainAuctionRecord) {
    writer.write_struct(|s| {
        s.field_string("domain", &record.domain);
        s.field_option_string("seller_account", record.seller_account.as_deref());
        s.field_option_string("seller_stake", record.seller_stake.as_deref());
        s.field_with("protocol_fee_bps", |w| w.write_u16(record.protocol_fee_bps));
        s.field_with("royalty_bps", |w| w.write_u16(record.royalty_bps));
        s.field_u64("min_bid_ct", record.min_bid_ct);
        s.field_u64("stake_requirement_ct", record.stake_requirement_ct);
        s.field_u64("start_ts", record.start_ts);
        s.field_u64("end_ts", record.end_ts);
        s.field_u8("status", status_to_u8(record.status));
        s.field_with("highest_bid", |w| {
            w.write_option_with(record.highest_bid.as_ref(), write_bid)
        });
        s.field_vec_with("bids", &record.bids, write_bid);
    });
}

fn read_auction(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainAuctionRecord> {
    let mut domain = None;
    let mut seller_account: Option<Option<String>> = None;
    let mut seller_stake: Option<Option<String>> = None;
    let mut protocol_fee_bps = None;
    let mut royalty_bps = None;
    let mut min_bid_ct = None;
    let mut stake_requirement_ct = None;
    let mut start_ts = None;
    let mut end_ts = None;
    let mut status = None;
    let mut highest_bid: Option<Option<DomainBidRecord>> = None;
    let mut bids: Option<Vec<DomainBidRecord>> = None;

    decode_struct(reader, Some(AUCTION_FIELD_COUNT), |key, reader| match key {
        "domain" => {
            let value = reader.read_string()?;
            assign_once(&mut domain, value, "domain")
        }
        "seller_account" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut seller_account, value, "seller_account")
        }
        "seller_stake" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut seller_stake, value, "seller_stake")
        }
        "protocol_fee_bps" => {
            let value = reader.read_u16()?;
            assign_once(&mut protocol_fee_bps, value, "protocol_fee_bps")
        }
        "royalty_bps" => {
            let value = reader.read_u16()?;
            assign_once(&mut royalty_bps, value, "royalty_bps")
        }
        "min_bid_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut min_bid_ct, value, "min_bid_ct")
        }
        "stake_requirement_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut stake_requirement_ct, value, "stake_requirement_ct")
        }
        "start_ts" => {
            let value = reader.read_u64()?;
            assign_once(&mut start_ts, value, "start_ts")
        }
        "end_ts" => {
            let value = reader.read_u64()?;
            assign_once(&mut end_ts, value, "end_ts")
        }
        "status" => {
            let raw = reader.read_u8()?;
            let value = status_from_u8(raw)?;
            assign_once(&mut status, value, "status")
        }
        "highest_bid" => {
            let value = reader.read_option_with(|r| read_bid(r))?;
            assign_once(&mut highest_bid, value, "highest_bid")
        }
        "bids" => {
            let value = reader.read_vec_with(|r| read_bid(r))?;
            assign_once(&mut bids, value, "bids")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainAuctionRecord {
        domain: domain.ok_or(DecodeError::MissingField("domain"))?,
        seller_account: seller_account.unwrap_or(None),
        seller_stake: seller_stake.unwrap_or(None),
        protocol_fee_bps: protocol_fee_bps.ok_or(DecodeError::MissingField("protocol_fee_bps"))?,
        royalty_bps: royalty_bps.ok_or(DecodeError::MissingField("royalty_bps"))?,
        min_bid_ct: min_bid_ct.ok_or(DecodeError::MissingField("min_bid_ct"))?,
        stake_requirement_ct: stake_requirement_ct
            .ok_or(DecodeError::MissingField("stake_requirement_ct"))?,
        start_ts: start_ts.ok_or(DecodeError::MissingField("start_ts"))?,
        end_ts: end_ts.ok_or(DecodeError::MissingField("end_ts"))?,
        status: status.ok_or(DecodeError::MissingField("status"))?,
        highest_bid: highest_bid.unwrap_or(None),
        bids: bids.unwrap_or_default(),
    })
}

fn write_ownership(writer: &mut BinaryWriter, record: &DomainOwnershipRecord) {
    writer.write_struct(|s| {
        s.field_string("domain", &record.domain);
        s.field_string("owner_account", &record.owner_account);
        s.field_u64("acquired_at", record.acquired_at);
        s.field_with("royalty_bps", |w| w.write_u16(record.royalty_bps));
        s.field_u64("last_sale_price_ct", record.last_sale_price_ct);
    });
}

fn read_ownership(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainOwnershipRecord> {
    let mut domain = None;
    let mut owner_account = None;
    let mut acquired_at = None;
    let mut royalty_bps = None;
    let mut last_sale_price = None;

    decode_struct(
        reader,
        Some(OWNERSHIP_FIELD_COUNT),
        |key, reader| match key {
            "domain" => {
                let value = reader.read_string()?;
                assign_once(&mut domain, value, "domain")
            }
            "owner_account" => {
                let value = reader.read_string()?;
                assign_once(&mut owner_account, value, "owner_account")
            }
            "acquired_at" => {
                let value = reader.read_u64()?;
                assign_once(&mut acquired_at, value, "acquired_at")
            }
            "royalty_bps" => {
                let value = reader.read_u16()?;
                assign_once(&mut royalty_bps, value, "royalty_bps")
            }
            "last_sale_price_ct" => {
                let value = reader.read_u64()?;
                assign_once(&mut last_sale_price, value, "last_sale_price_ct")
            }
            other => Err(DecodeError::UnknownField(other.to_owned())),
        },
    )?;

    Ok(DomainOwnershipRecord {
        domain: domain.ok_or(DecodeError::MissingField("domain"))?,
        owner_account: owner_account.ok_or(DecodeError::MissingField("owner_account"))?,
        acquired_at: acquired_at.ok_or(DecodeError::MissingField("acquired_at"))?,
        royalty_bps: royalty_bps.ok_or(DecodeError::MissingField("royalty_bps"))?,
        last_sale_price_ct: last_sale_price
            .ok_or(DecodeError::MissingField("last_sale_price_ct"))?,
    })
}

fn write_sale(writer: &mut BinaryWriter, record: &DomainSaleRecord) {
    writer.write_struct(|s| {
        s.field_string("domain", &record.domain);
        s.field_option_string("seller_account", record.seller_account.as_deref());
        s.field_string("buyer_account", &record.buyer_account);
        s.field_u64("sold_at", record.sold_at);
        s.field_u64("price_ct", record.price_ct);
        s.field_u64("protocol_fee_ct", record.protocol_fee_ct);
        s.field_u64("royalty_fee_ct", record.royalty_fee_ct);
    });
}

fn read_sale(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainSaleRecord> {
    let mut domain = None;
    let mut seller_account: Option<Option<String>> = None;
    let mut buyer_account = None;
    let mut sold_at = None;
    let mut price = None;
    let mut protocol_fee = None;
    let mut royalty_fee = None;

    decode_struct(reader, Some(SALE_FIELD_COUNT), |key, reader| match key {
        "domain" => {
            let value = reader.read_string()?;
            assign_once(&mut domain, value, "domain")
        }
        "seller_account" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut seller_account, value, "seller_account")
        }
        "buyer_account" => {
            let value = reader.read_string()?;
            assign_once(&mut buyer_account, value, "buyer_account")
        }
        "sold_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut sold_at, value, "sold_at")
        }
        "price_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut price, value, "price_ct")
        }
        "protocol_fee_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut protocol_fee, value, "protocol_fee_ct")
        }
        "royalty_fee_ct" => {
            let value = reader.read_u64()?;
            assign_once(&mut royalty_fee, value, "royalty_fee_ct")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainSaleRecord {
        domain: domain.ok_or(DecodeError::MissingField("domain"))?,
        seller_account: seller_account.unwrap_or(None),
        buyer_account: buyer_account.ok_or(DecodeError::MissingField("buyer_account"))?,
        sold_at: sold_at.ok_or(DecodeError::MissingField("sold_at"))?,
        price_ct: price.ok_or(DecodeError::MissingField("price_ct"))?,
        protocol_fee_ct: protocol_fee.ok_or(DecodeError::MissingField("protocol_fee_ct"))?,
        royalty_fee_ct: royalty_fee.ok_or(DecodeError::MissingField("royalty_fee_ct"))?,
    })
}

fn map_decode_error(err: DecodeError) -> AuctionError {
    #[cfg(test)]
    {
        eprintln!("dns decode error: {err}");
    }
    let _ = err;
    AuctionError::Storage
}

pub fn list_for_sale(params: &Value) -> Result<Value, AuctionError> {
    let domain = params
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let min_bid = params
        .get("min_bid_ct")
        .and_then(|v| v.as_u64())
        .ok_or(AuctionError::BidTooLow)?;
    if min_bid == 0 {
        return Err(AuctionError::BidTooLow);
    }
    let mut stake_requirement = params
        .get("stake_requirement_ct")
        .and_then(|v| v.as_u64())
        .unwrap_or(min_bid);
    if stake_requirement < min_bid {
        stake_requirement = min_bid;
    }
    let duration_secs = params
        .get("duration_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(86_400);
    let mut royalty_bps_param = params
        .get("royalty_bps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if royalty_bps_param > 10_000 {
        royalty_bps_param = 10_000;
    }
    let seller_account = params
        .get("seller_account")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let seller_stake = params
        .get("seller_stake")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    ensure_domain_allowed(domain, &db)?;

    let mut prior_record = None;
    if let Some(bytes) = db.get(&auction_key(domain)) {
        let existing = decode_auction(&bytes)?;
        if existing.status == AuctionStatus::Active {
            return Err(AuctionError::ListingActive);
        }
        prior_record = Some(existing);
    }

    let mut protocol_fee_bps = params
        .get("protocol_fee_bps")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            prior_record
                .as_ref()
                .map(|record| record.protocol_fee_bps as u64)
        })
        .unwrap_or(500);
    if protocol_fee_bps > 10_000 {
        protocol_fee_bps = 10_000;
    }

    let ownership = db
        .get(&ownership_key(domain))
        .map(|bytes| decode_ownership(&bytes))
        .transpose()?;

    let mut royalty_bps = royalty_bps_param as u16;
    if let Some(owner) = ownership {
        royalty_bps = owner.royalty_bps;
        match seller_account.as_ref() {
            Some(seller) if seller == &owner.owner_account => {}
            _ => return Err(AuctionError::OwnershipMismatch),
        }
    }

    let start_ts = now_ts();
    let end_ts = start_ts.saturating_add(duration_secs);

    let record = DomainAuctionRecord {
        domain: domain.to_string(),
        seller_account: seller_account.clone(),
        seller_stake,
        protocol_fee_bps: protocol_fee_bps as u16,
        royalty_bps,
        min_bid_ct: min_bid,
        stake_requirement_ct: stake_requirement,
        start_ts,
        end_ts,
        status: AuctionStatus::Active,
        highest_bid: None,
        bids: Vec::new(),
    };

    let bytes = encode_auction(&record)?;
    db.insert(&auction_key(domain), bytes);

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("auction", auction_to_json(&record)),
    ]))
}

pub fn place_bid(params: &Value) -> Result<Value, AuctionError> {
    let domain = params
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let bidder = params
        .get("bidder_account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if bidder.is_empty() {
        return Err(AuctionError::InvalidBidder);
    }
    let amount = params
        .get("bid_ct")
        .and_then(|v| v.as_u64())
        .ok_or(AuctionError::BidTooLow)?;
    if amount == 0 {
        return Err(AuctionError::BidTooLow);
    }
    let stake_reference = params
        .get("stake_reference")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let key = auction_key(domain);
    let mut record = match db.get(&key) {
        Some(bytes) => decode_auction(&bytes)?,
        None => return Err(AuctionError::AuctionMissing),
    };

    if record.status != AuctionStatus::Active {
        return Err(AuctionError::AuctionClosed);
    }
    let now = now_ts();
    if now >= record.end_ts {
        return Err(AuctionError::AuctionExpired);
    }
    if amount < record.min_bid_ct {
        return Err(AuctionError::BidTooLow);
    }
    if amount < record.stake_requirement_ct {
        return Err(AuctionError::BidInsufficientStake);
    }
    if let Some(highest) = record.highest_bid.as_ref() {
        if amount <= highest.amount_ct {
            return Err(AuctionError::BidTooLow);
        }
    }

    let bid = DomainBidRecord {
        bidder: bidder.to_string(),
        amount_ct: amount,
        stake_reference,
        placed_at: now,
    };
    record.highest_bid = Some(bid.clone());
    record.bids.push(bid);

    let bytes = encode_auction(&record)?;
    db.insert(&key, bytes);

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("auction", auction_to_json(&record)),
    ]))
}

pub fn complete_sale(params: &Value) -> Result<Value, AuctionError> {
    let domain = params
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let force = params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let key = auction_key(domain);
    let mut record = match db.get(&key) {
        Some(bytes) => decode_auction(&bytes)?,
        None => return Err(AuctionError::AuctionMissing),
    };

    if record.status != AuctionStatus::Active {
        return Err(AuctionError::AuctionClosed);
    }
    let now = now_ts();
    if !force && now < record.end_ts {
        return Err(AuctionError::AuctionNotFinished);
    }

    let winning_bid = match record.highest_bid.clone() {
        Some(bid) => bid,
        None => {
            record.status = AuctionStatus::Cancelled;
            let bytes = encode_auction(&record)?;
            db.insert(&key, bytes);
            return Err(AuctionError::NoBids);
        }
    };

    let protocol_fee_ct = winning_bid
        .amount_ct
        .saturating_mul(record.protocol_fee_bps as u64)
        / 10_000;

    let royalty_fee_ct = winning_bid
        .amount_ct
        .saturating_mul(record.royalty_bps as u64)
        / 10_000;

    record_treasury_fee(protocol_fee_ct.saturating_add(royalty_fee_ct));

    let ownership_key = ownership_key(domain);
    let ownership = db
        .get(&ownership_key)
        .map(|bytes| decode_ownership(&bytes))
        .transpose()?;

    let new_owner = DomainOwnershipRecord {
        domain: domain.to_string(),
        owner_account: winning_bid.bidder.clone(),
        acquired_at: now,
        royalty_bps: ownership
            .as_ref()
            .map(|o| o.royalty_bps)
            .unwrap_or(record.royalty_bps),
        last_sale_price_ct: winning_bid.amount_ct,
    };
    let ownership_bytes = encode_ownership(&new_owner)?;
    db.insert(&ownership_key, ownership_bytes);

    let mut history = db
        .get(&sale_history_key(domain))
        .map(|bytes| decode_sales(&bytes))
        .transpose()?
        .unwrap_or_default();
    history.push(DomainSaleRecord {
        domain: domain.to_string(),
        sold_at: now,
        seller_account: record.seller_account.clone(),
        buyer_account: winning_bid.bidder.clone(),
        price_ct: winning_bid.amount_ct,
        protocol_fee_ct,
        royalty_fee_ct,
    });
    let history_bytes = encode_sales(&history)?;
    db.insert(&sale_history_key(domain), history_bytes);

    record.status = AuctionStatus::Settled;
    record.end_ts = now;
    let bytes = encode_auction(&record)?;
    db.insert(&key, bytes);

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        (
            "sale",
            json_map(vec![
                ("domain", Value::String(domain.to_string())),
                ("buyer_account", Value::String(winning_bid.bidder.clone())),
                (
                    "price_ct",
                    Value::Number(Number::from(winning_bid.amount_ct)),
                ),
                (
                    "protocol_fee_ct",
                    Value::Number(Number::from(protocol_fee_ct)),
                ),
                (
                    "royalty_fee_ct",
                    Value::Number(Number::from(royalty_fee_ct)),
                ),
            ]),
        ),
        ("ownership", ownership_to_json(&new_owner)),
    ]))
}

pub fn auctions(params: &Value) -> Result<Value, AuctionError> {
    let filter = params
        .get("domain")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let mut auctions = Vec::new();
    let mut ownerships = Vec::new();
    let mut history = Vec::new();

    let domains: Vec<String> = if let Some(domain) = filter.clone() {
        vec![domain]
    } else {
        db.keys_with_prefix("dns_auction/")
            .into_iter()
            .filter_map(|key| key.strip_prefix("dns_auction/").map(|s| s.to_string()))
            .collect()
    };

    for domain in domains {
        if let Some(bytes) = db.get(&auction_key(&domain)) {
            let record = decode_auction(&bytes)?;
            auctions.push(auction_to_json(&record));
        }
        if let Some(bytes) = db.get(&ownership_key(&domain)) {
            let record = decode_ownership(&bytes)?;
            ownerships.push(ownership_to_json(&record));
        }
        if let Some(bytes) = db.get(&sale_history_key(&domain)) {
            let records = decode_sales(&bytes)?;
            let values: Vec<Value> = records.iter().map(sale_to_json).collect();
            history.push((domain.clone(), Value::Array(values)));
        }
    }

    let history_value = Value::Array(
        history
            .into_iter()
            .map(|(domain, records)| {
                json_map(vec![
                    ("domain", Value::String(domain)),
                    ("records", records),
                ])
            })
            .collect(),
    );

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("auctions", Value::Array(auctions)),
        ("ownership", Value::Array(ownerships)),
        ("history", history_value),
    ]))
}

fn json_map(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn default_txt_resolver(domain: &str) -> Vec<String> {
    let mut delay = Duration::from_millis(100);
    for _ in 0..3 {
        if let Ok(records) = lookup_txt(domain) {
            return records;
        }
        thread::sleep(delay);
        delay *= 2;
    }
    Vec::new()
}

pub fn set_allow_external(val: bool) {
    ALLOW_EXTERNAL.store(val, Ordering::Relaxed);
}

pub fn set_disable_verify(val: bool) {
    DISABLE_VERIFY.store(val, Ordering::Relaxed);
}

pub fn set_txt_resolver<F>(f: F)
where
    F: Fn(&str) -> Vec<String> + Send + Sync + 'static,
{
    *TXT_RESOLVER.lock().unwrap() = Box::new(f);
}

pub fn clear_verify_cache() {
    VERIFY_CACHE.lock().unwrap().clear();
}

pub enum DnsError {
    SigInvalid,
}

impl DnsError {
    pub fn code(&self) -> i32 {
        -(ERR_DNS_SIG_INVALID as i32)
    }
    pub fn message(&self) -> &'static str {
        "ERR_DNS_SIG_INVALID"
    }
}

pub fn publish_record(params: &Value) -> Result<Value, DnsError> {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let txt = params.get("txt").and_then(|v| v.as_str()).unwrap_or("");
    let pk_hex = params.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
    let sig_hex = params.get("sig").and_then(|v| v.as_str()).unwrap_or("");
    let pk_vec = crypto_suite::hex::decode(pk_hex)
        .ok()
        .ok_or(DnsError::SigInvalid)?;
    let sig_vec = crypto_suite::hex::decode(sig_hex)
        .ok()
        .ok_or(DnsError::SigInvalid)?;
    let pk: [u8; PUBLIC_KEY_LENGTH] = pk_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| DnsError::SigInvalid)?;
    let sig = Signature::from_bytes(&sig_bytes);
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    vk.verify(&msg, &sig).map_err(|_| DnsError::SigInvalid)?;
    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    db.insert(&format!("dns_records/{}", domain), txt.as_bytes().to_vec());
    db.insert(&format!("dns_keys/{}", domain), pk_hex.as_bytes().to_vec());
    db.insert(
        &format!("dns_reads/{}", domain),
        0u64.to_le_bytes().to_vec(),
    );
    db.insert(&format!("dns_last/{}", domain), 0u64.to_le_bytes().to_vec());
    mobile_cache::purge_policy(domain);
    Ok(json_map(vec![("status", Value::String("ok".to_string()))]))
}

pub fn verify_txt(domain: &str, node_id: &str) -> bool {
    if DISABLE_VERIFY.load(Ordering::Relaxed) {
        return true;
    }
    if domain.ends_with(".block") {
        return true;
    }
    if !ALLOW_EXTERNAL.load(Ordering::Relaxed) {
        return false;
    }
    let key = format!("{}:{}", domain, node_id);
    let now = Instant::now();
    if let Some((ok, ts)) = VERIFY_CACHE.lock().unwrap().get(&key) {
        if now.duration_since(*ts) < VERIFY_TTL {
            return *ok;
        }
    }
    let txts = {
        let resolver = TXT_RESOLVER.lock().unwrap();
        resolver(domain)
    };
    let needle = format!("tb-verification={}", node_id);
    let ok = txts.iter().any(|t| t.contains(&needle));
    VERIFY_CACHE.lock().unwrap().insert(key, (ok, now));
    #[cfg(feature = "telemetry")]
    {
        let status = if ok { "verified" } else { "rejected" };
        GATEWAY_DNS_LOOKUP_TOTAL
            .ensure_handle_for_label_values(&[status])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .inc();
        if !ok {
            DNS_VERIFICATION_FAIL_TOTAL.inc();
        }
    }
    if !ok {
        #[cfg(feature = "telemetry")]
        warn!(%domain, "gateway dns verification failed");
    }
    ok
}

pub fn gateway_policy(params: &Value) -> Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let key = format!("dns_records/{}", domain);
    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(bytes) = db.get(&key) {
        if let Ok(txt) = String::from_utf8(bytes) {
            let pk = db
                .get(&format!("dns_keys/{}", domain))
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default();
            if verify_txt(domain, &pk) {
                let reads_key = format!("dns_reads/{}", domain);
                let last_key = format!("dns_last/{}", domain);
                let mut reads = db
                    .get(&reads_key)
                    .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
                    .unwrap_or(0);
                reads += 1;
                db.insert(&reads_key, reads.to_le_bytes().to_vec());
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                db.insert(&last_key, ts.to_le_bytes().to_vec());
                let _ = read_receipt::append(domain, "gateway", txt.len() as u64, false, true);
                let response = json_map(vec![
                    ("record", Value::String(txt.clone())),
                    ("reads_total", Value::Number(Number::from(reads))),
                    ("last_access_ts", Value::Number(Number::from(ts))),
                ]);
                mobile_cache::cache_policy(domain, &response);
                return response;
            }
        }
    }
    if let Some(cached) = mobile_cache::cached_policy(domain) {
        return cached;
    }
    let miss = json_map(vec![
        ("record", Value::Null),
        ("reads_total", Value::Number(Number::from(0))),
        ("last_access_ts", Value::Number(Number::from(0))),
    ]);
    mobile_cache::cache_policy(domain, &miss);
    miss
}

pub fn reads_since(params: &Value) -> Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let epoch = params.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0);
    let (total, last) = read_receipt::reads_since(epoch, domain);
    json_map(vec![
        ("reads_total", Value::Number(Number::from(total))),
        ("last_access_ts", Value::Number(Number::from(last))),
    ])
}

pub fn dns_lookup(params: &Value) -> Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let txt = db
        .get(&format!("dns_records/{}", domain))
        .and_then(|v| String::from_utf8(v).ok());
    let pk = db
        .get(&format!("dns_keys/{}", domain))
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or_default();
    let verified = txt
        .as_ref()
        .map(|_| verify_txt(domain, &pk))
        .unwrap_or(false);
    json_map(vec![
        ("record", txt.map(Value::String).unwrap_or(Value::Null)),
        ("verified", Value::Bool(verified)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::{Number, Value};
    use std::sync::{Arc, Mutex};

    fn clear_domain_state(domain: &str) {
        let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let keys = [
            auction_key(domain),
            ownership_key(domain),
            sale_history_key(domain),
            format!("dns_records/{domain}"),
            format!("dns_keys/{domain}"),
        ];
        for key in keys {
            db.remove(&key);
        }
    }

    #[testkit::tb_serial]
    fn premium_domain_primary_sale_flow() {
        let domain = "premium-test.block";
        clear_domain_state(domain);

        let captured = Arc::new(Mutex::new(Vec::new()));
        let hook_capture = Arc::clone(&captured);
        install_treasury_hook(move |amount| {
            hook_capture.lock().unwrap().push(amount);
        });

        let listing = list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid_ct", Value::Number(Number::from(1_000))),
            ("protocol_fee_bps", Value::Number(Number::from(500))),
            ("royalty_bps", Value::Number(Number::from(200))),
        ]))
        .expect("listing ok");
        assert_eq!(listing["status"].as_str(), Some("ok"));

        let low_bid = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-low".to_string())),
            ("bid_ct", Value::Number(Number::from(800))),
        ]));
        assert!(matches!(low_bid, Err(AuctionError::BidTooLow)));

        let winning_bid = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-main".to_string())),
            ("bid_ct", Value::Number(Number::from(1_500))),
            ("stake_reference", Value::String("stake-1".to_string())),
        ]))
        .expect("winning bid");
        assert_eq!(winning_bid["status"].as_str(), Some("ok"));

        let sale = complete_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("force", Value::Bool(true)),
        ]))
        .expect("sale completes");
        assert_eq!(sale["status"].as_str(), Some("ok"));
        assert_eq!(sale["sale"]["price_ct"].as_u64(), Some(1_500));

        let treasury = captured.lock().unwrap().clone();
        assert_eq!(treasury, vec![105]);
        clear_treasury_hook();

        let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let owner_bytes = db.get(&ownership_key(domain)).expect("ownership stored");
        let owner = decode_ownership(&owner_bytes).expect("decode owner");
        assert_eq!(owner.owner_account, "bidder-main");
        assert_eq!(owner.royalty_bps, 200);

        let history_bytes = db.get(&sale_history_key(domain)).expect("history stored");
        let history = decode_sales(&history_bytes).expect("decode history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].buyer_account, "bidder-main");
        drop(db);

        clear_domain_state(domain);
    }

    #[testkit::tb_serial]
    fn resale_respects_royalty_distribution() {
        let domain = "resale-test.block";
        clear_domain_state(domain);

        install_treasury_hook(|_| {});
        // Seed primary sale to establish ownership and royalty rate.
        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid_ct", Value::Number(Number::from(2_000))),
            ("protocol_fee_bps", Value::Number(Number::from(400))),
            ("royalty_bps", Value::Number(Number::from(150))),
        ]))
        .expect("primary listing");
        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("first-owner".to_string())),
            ("bid_ct", Value::Number(Number::from(2_500))),
        ]))
        .expect("primary winning bid");
        complete_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("force", Value::Bool(true)),
        ]))
        .expect("primary sale");
        clear_treasury_hook();

        let captured = Arc::new(Mutex::new(Vec::new()));
        let hook_capture = Arc::clone(&captured);
        install_treasury_hook(move |amount| {
            hook_capture.lock().unwrap().push(amount);
        });

        let resale_listing = list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid_ct", Value::Number(Number::from(3_000))),
            ("seller_account", Value::String("first-owner".to_string())),
            // Intentionally set royalty to a different value to ensure the stored value persists.
            ("royalty_bps", Value::Number(Number::from(0))),
        ]))
        .expect("resale listing");
        assert_eq!(resale_listing["status"].as_str(), Some("ok"));

        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("second-owner".to_string())),
            ("bid_ct", Value::Number(Number::from(3_600))),
        ]))
        .expect("resale winning bid");

        complete_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("force", Value::Bool(true)),
        ]))
        .expect("resale sale");

        let treasury = captured.lock().unwrap().clone();
        // Protocol fee: 3,600 * 4% = 144; royalty: 3,600 * 1.5% = 54; total 198.
        assert_eq!(treasury, vec![198]);
        clear_treasury_hook();

        let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let owner_bytes = db.get(&ownership_key(domain)).expect("ownership stored");
        let owner = decode_ownership(&owner_bytes).expect("decode owner");
        assert_eq!(owner.owner_account, "second-owner");
        assert_eq!(owner.royalty_bps, 150);
        let history_bytes = db.get(&sale_history_key(domain)).expect("history stored");
        let history = decode_sales(&history_bytes).expect("decode history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].buyer_account, "second-owner");
        drop(db);

        clear_domain_state(domain);
    }

    #[testkit::tb_serial]
    fn bid_rejected_after_expiry() {
        let domain = "expiry-test.block";
        clear_domain_state(domain);

        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid_ct", Value::Number(Number::from(500))),
            ("duration_secs", Value::Number(Number::from(0))),
        ]))
        .expect("listing");

        let result = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("late-bid".to_string())),
            ("bid_ct", Value::Number(Number::from(600))),
        ]));
        assert!(matches!(result, Err(AuctionError::AuctionExpired)));

        clear_domain_state(domain);
    }
}
