#![forbid(unsafe_code)]

use crate::energy::{self, DisputeError, DisputeFilter, DisputeStatus};
use crypto_suite::hex;
use energy_market::{
    EnergyCredit, EnergyMarketError, EnergyProvider, EnergyReceipt, MeterReading, H256,
};
use foundation_rpc::{Params, RpcError};
use foundation_serialization::json::{Map, Number, Value};
use governance_spec::EnergySettlementMode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const ERR_SIGNATURE_INVALID: i32 = -33001;
const ERR_METER_MISMATCH: i32 = -33003;
const ERR_QUORUM_FAILED: i32 = -33004;
const ERR_SETTLEMENT_CONFLICT: i32 = -33005;
const ERR_PROVIDER_INACTIVE: i32 = -33006;
const ERR_NONCE_REPLAY: i32 = -33007;
const ERR_TIMESTAMP_SKEW: i32 = -33008;
const ERR_AUTH_REQUIRED: i32 = -33009;
const ERR_RATE_LIMIT: i32 = -33010;
const ERR_INVALID_PARAMS: i32 = -32602;

static RATE_WINDOW_START: AtomicU64 = AtomicU64::new(0);
static RATE_WINDOW_COUNT: AtomicU64 = AtomicU64::new(0);

fn rate_limit_limit() -> u64 {
    std::env::var("TB_ENERGY_RPC_RPS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(50)
}

fn enforce_rate_limit() -> Result<(), RpcError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let window = RATE_WINDOW_START.load(Ordering::Relaxed);
    let limit = rate_limit_limit();
    if window == now {
        let count = RATE_WINDOW_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if count > limit {
            return Err(energy_rpc_error(
                ERR_RATE_LIMIT,
                format!("rate limit exceeded ({count}/{limit} rps)"),
            ));
        }
    } else {
        RATE_WINDOW_START.store(now, Ordering::Relaxed);
        RATE_WINDOW_COUNT.store(1, Ordering::Relaxed);
    }
    Ok(())
}

fn enforce_auth(params: &Map) -> Result<(), RpcError> {
    let Some(token) = std::env::var("TB_ENERGY_RPC_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
    else {
        return Ok(()); // Auth not configured
    };
    let provided = params.get("auth_token").and_then(|v| v.as_str());
    if provided != Some(token.as_str()) {
        return Err(energy_rpc_error(
            ERR_AUTH_REQUIRED,
            "energy RPC authentication failed",
        ));
    }
    Ok(())
}

fn energy_rpc_error(code: i32, message: impl Into<String>) -> RpcError {
    RpcError::new(code, message)
}

fn invalid_params(message: impl Into<String>) -> RpcError {
    energy_rpc_error(ERR_INVALID_PARAMS, message)
}

fn number(value: u64) -> Value {
    Value::Number(Number::from(value))
}

fn provider_value(provider: &EnergyProvider) -> Value {
    let mut map = Map::new();
    map.insert(
        "provider_id".into(),
        Value::String(provider.provider_id.clone()),
    );
    map.insert("owner".into(), Value::String(provider.owner.clone()));
    map.insert(
        "jurisdiction".into(),
        Value::String(provider.location.clone()),
    );
    map.insert("capacity_kwh".into(), number(provider.capacity_kwh));
    map.insert("available_kwh".into(), number(provider.available_kwh));
    map.insert("price_per_kwh".into(), number(provider.price_per_kwh));
    map.insert(
        "reputation_score".into(),
        Value::Number(Number::from_f64(provider.reputation_score).unwrap_or(Number::from(0u64))),
    );
    map.insert(
        "meter_address".into(),
        Value::String(provider.meter_address.clone()),
    );
    map.insert(
        "total_delivered_kwh".into(),
        number(provider.total_delivered_kwh),
    );
    map.insert("staked_balance".into(), number(provider.staked_balance));
    Value::Object(map)
}

fn receipt_value(receipt: &EnergyReceipt) -> Value {
    let mut map = Map::new();
    map.insert("buyer".into(), Value::String(receipt.buyer.clone()));
    map.insert("seller".into(), Value::String(receipt.seller.clone()));
    map.insert("kwh_delivered".into(), number(receipt.kwh_delivered));
    map.insert("price_paid".into(), number(receipt.price_paid));
    map.insert("block_settled".into(), number(receipt.block_settled));
    map.insert("treasury_fee".into(), number(receipt.treasury_fee));
    map.insert("slash_applied".into(), number(receipt.slash_applied));
    map.insert(
        "meter_hash".into(),
        Value::String(hex::encode(receipt.meter_reading_hash)),
    );
    Value::Object(map)
}

fn credit_value(credit: &EnergyCredit) -> Value {
    let mut map = Map::new();
    map.insert("provider".into(), Value::String(credit.provider.clone()));
    map.insert("amount_kwh".into(), number(credit.amount_kwh));
    map.insert("timestamp".into(), number(credit.timestamp));
    map.insert(
        "meter_hash".into(),
        Value::String(hex::encode(credit.meter_reading_hash)),
    );
    Value::Object(map)
}

fn dispute_value(dispute: &energy::EnergyDispute) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), number(dispute.id));
    map.insert(
        "provider_id".into(),
        Value::String(dispute.provider_id.clone()),
    );
    map.insert(
        "meter_hash".into(),
        Value::String(hex::encode(dispute.meter_hash)),
    );
    map.insert("reporter".into(), Value::String(dispute.reporter.clone()));
    map.insert("reason".into(), Value::String(dispute.reason.clone()));
    map.insert(
        "status".into(),
        Value::String(dispute_status_label(dispute.status).into()),
    );
    map.insert("opened_at".into(), number(dispute.opened_at));
    map.insert(
        "resolved_at".into(),
        dispute.resolved_at.map(number).unwrap_or(Value::Null),
    );
    map.insert(
        "resolution_note".into(),
        dispute
            .resolution_note
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "resolver".into(),
        dispute
            .resolver
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    Value::Object(map)
}

fn params_object(params: &Params) -> Result<&Map, RpcError> {
    let map = params
        .as_map()
        .ok_or_else(|| invalid_params("parameters must be an object"))?;
    enforce_rate_limit()?;
    enforce_auth(map)?;
    Ok(map)
}

fn require_string(params: &Map, key: &str) -> Result<String, RpcError> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| invalid_params(format!("missing or invalid '{key}'")))
}

fn require_u64(params: &Map, key: &str) -> Result<u64, RpcError> {
    params
        .get(key)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| invalid_params(format!("missing or invalid '{key}'")))
}

fn optional_u64(params: &Map, key: &str) -> Option<u64> {
    params.get(key).and_then(|value| value.as_u64())
}

fn optional_string(params: &Map, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn decode_hash(hex_value: &str) -> Result<H256, RpcError> {
    let bytes = hex::decode(hex_value)
        .map_err(|_| invalid_params("meter hash must be hex-encoded 32 bytes"))?;
    if bytes.len() != 32 {
        return Err(invalid_params("meter hash must be 32 bytes"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn decode_signature(hex_value: &str) -> Result<Vec<u8>, RpcError> {
    let bytes =
        hex::decode(hex_value).map_err(|_| invalid_params("signature must be hex encoded"))?;
    // Energy RPC currently supports Ed25519-only signatures; enforcement here keeps the error
    // contract explicit before verifier logic runs.
    const ED25519_SIG_LEN: usize = 64;
    if bytes.len() != ED25519_SIG_LEN {
        return Err(invalid_params(format!(
            "signature must be {ED25519_SIG_LEN} bytes"
        )));
    }
    Ok(bytes)
}

fn dispute_status_from_str(label: &str) -> Option<DisputeStatus> {
    match label {
        "open" => Some(DisputeStatus::Open),
        "resolved" => Some(DisputeStatus::Resolved),
        _ => None,
    }
}

fn dispute_status_label(status: DisputeStatus) -> &'static str {
    match status {
        DisputeStatus::Open => "open",
        DisputeStatus::Resolved => "resolved",
    }
}

fn settlement_mode_label(mode: EnergySettlementMode) -> &'static str {
    match mode {
        EnergySettlementMode::Batch => "batch",
        EnergySettlementMode::RealTime => "real_time",
    }
}

fn map_energy_error(err: EnergyMarketError) -> RpcError {
    match err {
        EnergyMarketError::ProviderExists { provider_id } => energy_rpc_error(
            ERR_PROVIDER_INACTIVE,
            format!("provider already registered: {provider_id}"),
        ),
        EnergyMarketError::MeterAddressInUse { meter_address } => energy_rpc_error(
            ERR_PROVIDER_INACTIVE,
            format!("meter address already claimed: {meter_address}"),
        ),
        EnergyMarketError::InsufficientStake { stake, min } => energy_rpc_error(
            ERR_PROVIDER_INACTIVE,
            format!("stake {stake} below required minimum {min}"),
        ),
        EnergyMarketError::InsufficientCapacity {
            provider_id,
            requested_kwh,
            available_kwh,
        } => energy_rpc_error(
            ERR_PROVIDER_INACTIVE,
            format!(
                "insufficient capacity for provider {provider_id}: requested {requested_kwh} kWh but only {available_kwh} kWh remain"
            ),
        ),
        EnergyMarketError::UnknownProvider(provider_id) => {
            energy_rpc_error(ERR_PROVIDER_INACTIVE, format!("unknown provider {provider_id}"))
        }
        EnergyMarketError::StaleReading { provider_id } => energy_rpc_error(
            ERR_METER_MISMATCH,
            format!("reading timestamp regression for provider {provider_id}"),
        ),
        EnergyMarketError::InvalidMeterValue { provider_id } => energy_rpc_error(
            ERR_METER_MISMATCH,
            format!("reading totalized kWh decreased for provider {provider_id}"),
        ),
        EnergyMarketError::UnknownReading(hash) => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("meter reading hash {hash:?} not tracked"),
        ),
        EnergyMarketError::InsufficientCredit {
            requested_kwh,
            available_kwh,
        } => energy_rpc_error(
            ERR_PROVIDER_INACTIVE,
            format!("requested {requested_kwh} kWh exceeds credit {available_kwh}"),
        ),
        EnergyMarketError::CreditExpired(hash) => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("meter reading {hash:?} expired"),
        ),
        EnergyMarketError::SignatureVerificationFailed(reason) => {
            energy_rpc_error(ERR_SIGNATURE_INVALID, format!("signature verification failed: {reason}"))
        }
        EnergyMarketError::SettlementNotDue { next_block } => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("settlement gated by batch policy until block {next_block}"),
        ),
        EnergyMarketError::SettlementBelowQuorum {
            required_ppm,
            actual_ppm,
        } => energy_rpc_error(
            ERR_QUORUM_FAILED,
            format!("settlement below quorum: required {required_ppm} ppm, actual {actual_ppm} ppm"),
        ),
        EnergyMarketError::NonceReplay { nonce, .. } => energy_rpc_error(
            ERR_NONCE_REPLAY,
            format!("nonce {nonce} already used for provider"),
        ),
        EnergyMarketError::TimestampSkew {
            tolerance_secs,
            observed_skew,
            ..
        } => energy_rpc_error(
            ERR_TIMESTAMP_SKEW,
            format!(
                "timestamp skew {observed_skew}s exceeds tolerance {tolerance_secs}s"
            ),
        ),
    }
}

fn map_dispute_error(err: DisputeError) -> RpcError {
    match err {
        DisputeError::UnknownMeterReading { meter_hash } => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("meter hash {meter_hash:?} not tracked for disputes"),
        ),
        DisputeError::AlreadyOpen { meter_hash } => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("dispute already open for meter hash {meter_hash:?}"),
        ),
        DisputeError::UnknownDispute { dispute_id } => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("dispute {dispute_id} not found"),
        ),
        DisputeError::AlreadyResolved { dispute_id } => energy_rpc_error(
            ERR_SETTLEMENT_CONFLICT,
            format!("dispute {dispute_id} already resolved"),
        ),
    }
}

pub fn register(params: &Params) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let capacity = require_u64(params, "capacity_kwh")?;
    let price = require_u64(params, "price_per_kwh")?;
    let stake = require_u64(params, "stake")?;
    let meter_address = require_string(params, "meter_address")?;
    let jurisdiction = require_string(params, "jurisdiction")?;
    let owner = require_string(params, "owner").unwrap_or_else(|_| "anonymous".into());
    match energy::register_provider(owner, capacity, price, meter_address, jurisdiction, stake) {
        Ok(provider) => Ok(provider_value(&provider)),
        Err(err) => Err(map_energy_error(err)),
    }
}

pub fn update_provider(params: &Params) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = require_string(params, "provider_id")?;
    let capacity = optional_u64(params, "capacity_kwh");
    let price = optional_u64(params, "price_per_kwh");
    let jurisdiction = optional_string(params, "jurisdiction");
    if capacity.is_none() && price.is_none() && jurisdiction.is_none() {
        return Err(invalid_params(
            "provide at least one of capacity_kwh, price_per_kwh, or jurisdiction",
        ));
    }
    match energy::update_provider(&provider_id, price, capacity, jurisdiction) {
        Ok(provider) => Ok(provider_value(&provider)),
        Err(err) => Err(map_energy_error(err)),
    }
}

pub fn market_state(filter_provider: Option<&str>) -> Result<Value, RpcError> {
    let snapshot = energy::market_snapshot();
    let energy::EnergySnapshot {
        providers,
        receipts,
        credits,
        disputes,
        governance,
        ..
    } = snapshot;
    let providers: Vec<Value> = providers
        .into_iter()
        .filter(|provider| {
            filter_provider
                .map(|target| provider.provider_id == target)
                .unwrap_or(true)
        })
        .map(|provider| provider_value(&provider))
        .collect();
    let credits: Vec<Value> = credits
        .into_iter()
        .filter(|credit| {
            filter_provider
                .map(|target| credit.provider == target)
                .unwrap_or(true)
        })
        .map(|credit| credit_value(&credit))
        .collect();
    let receipts: Vec<Value> = receipts
        .into_iter()
        .filter(|receipt| {
            filter_provider
                .map(|target| receipt.seller == target)
                .unwrap_or(true)
        })
        .map(|receipt| receipt_value(&receipt))
        .collect();
    let disputes: Vec<Value> = disputes
        .into_iter()
        .filter(|dispute| {
            filter_provider
                .map(|target| dispute.provider_id == target)
                .unwrap_or(true)
        })
        .map(|dispute| dispute_value(&dispute))
        .collect();
    let mut governance_map = Map::new();
    governance_map.insert(
        "mode".into(),
        Value::String(settlement_mode_label(governance.settlement.mode).into()),
    );
    governance_map.insert(
        "quorum_threshold_ppm".into(),
        Value::Number(Number::from(governance.settlement.quorum_threshold_ppm)),
    );
    governance_map.insert(
        "expiry_blocks".into(),
        Value::Number(Number::from(governance.settlement.expiry_blocks)),
    );
    governance_map.insert(
        "oracle_timeout_blocks".into(),
        Value::Number(Number::from(governance.oracle_timeout_blocks)),
    );
    governance_map.insert(
        "min_stake".into(),
        Value::Number(Number::from(governance.min_stake)),
    );
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert("providers".into(), Value::Array(providers));
    map.insert("credits".into(), Value::Array(credits));
    map.insert("receipts".into(), Value::Array(receipts));
    map.insert("disputes".into(), Value::Array(disputes));
    map.insert("governance".into(), Value::Object(governance_map));
    Ok(Value::Object(map))
}

pub fn disputes(params: &Params) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = params.get("provider_id").and_then(|value| value.as_str());
    let status = match params.get("status").and_then(|value| value.as_str()) {
        Some(label) => match dispute_status_from_str(label) {
            Some(status) => Some(status),
            None => {
                return Err(invalid_params(
                    "invalid dispute status (expected 'open' or 'resolved')",
                ))
            }
        },
        None => None,
    };
    let meter_hash = match params.get("meter_hash").and_then(|value| value.as_str()) {
        Some(hash) => match decode_hash(hash) {
            Ok(decoded) => Some(decoded),
            Err(err) => return Err(err),
        },
        None => None,
    };
    let page = params
        .get("page")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let page_size = params
        .get("page_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(25) as usize;
    let filter = DisputeFilter {
        provider_id,
        status,
        meter_hash,
    };
    let page = energy::disputes_page(filter, page, page_size);
    let disputes: Vec<Value> = page.items.iter().map(dispute_value).collect();
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert("page".into(), number(page.page as u64));
    map.insert("page_size".into(), number(page.page_size as u64));
    map.insert("total".into(), number(page.total as u64));
    map.insert("disputes".into(), Value::Array(disputes));
    Ok(Value::Object(map))
}

pub fn receipts(params: &Params) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = params.get("provider_id").and_then(|value| value.as_str());
    let page = params
        .get("page")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let page_size = params
        .get("page_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(25) as usize;
    let page = energy::receipts_page(provider_id, page, page_size);
    let receipts: Vec<Value> = page.items.iter().map(receipt_value).collect();
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert("page".into(), number(page.page as u64));
    map.insert("page_size".into(), number(page.page_size as u64));
    map.insert("total".into(), number(page.total as u64));
    map.insert("receipts".into(), Value::Array(receipts));
    Ok(Value::Object(map))
}

pub fn credits(params: &Params) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = params.get("provider_id").and_then(|value| value.as_str());
    let page = params
        .get("page")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let page_size = params
        .get("page_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(25) as usize;
    let page = energy::credits_page(provider_id, page, page_size);
    let credits: Vec<Value> = page.items.iter().map(credit_value).collect();
    let mut map = Map::new();
    map.insert("status".into(), Value::String("ok".into()));
    map.insert("page".into(), number(page.page as u64));
    map.insert("page_size".into(), number(page.page_size as u64));
    map.insert("total".into(), number(page.total as u64));
    map.insert("credits".into(), Value::Array(credits));
    Ok(Value::Object(map))
}

pub fn flag_dispute(params: &Params, block: u64) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let meter_hash = require_string(params, "meter_hash").and_then(|hex| decode_hash(&hex))?;
    let reason = require_string(params, "reason")?;
    let reporter = optional_string(params, "reporter").unwrap_or_else(|| "anonymous".into());
    match energy::flag_dispute(reporter, meter_hash, reason, block) {
        Ok(dispute) => Ok(dispute_value(&dispute)),
        Err(err) => Err(map_dispute_error(err)),
    }
}

pub fn resolve_dispute(params: &Params, block: u64) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let dispute_id = require_u64(params, "dispute_id")?;
    let resolver = optional_string(params, "resolver").unwrap_or_else(|| "system".into());
    let note = optional_string(params, "resolution_note");
    match energy::resolve_dispute(dispute_id, resolver, note, block) {
        Ok(dispute) => Ok(dispute_value(&dispute)),
        Err(err) => Err(map_dispute_error(err)),
    }
}

pub fn settle(params: &Params, block: u64) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = require_string(params, "provider_id")?;
    let buyer = require_string(params, "buyer").unwrap_or_else(|_| "anonymous".into());
    let kwh = require_u64(params, "kwh_consumed")?;
    let meter_hash = require_string(params, "meter_hash").and_then(|hash| decode_hash(&hash))?;
    match energy::settle_energy_delivery(buyer, &provider_id, kwh, block, meter_hash) {
        Ok(receipt) => Ok(receipt_value(&receipt)),
        Err(err) => Err(map_energy_error(err)),
    }
}

pub fn submit_reading(params: &Params, block: u64) -> Result<Value, RpcError> {
    let params = params_object(params)?;
    let provider_id = require_string(params, "provider_id")?;
    let meter_address = require_string(params, "meter_address")?;
    let kwh_reading = require_u64(params, "kwh_reading")?;
    let timestamp = require_u64(params, "timestamp")?;
    let nonce = require_u64(params, "nonce")?;
    let signature = require_string(params, "signature").and_then(|sig| decode_signature(&sig))?;
    let reading = MeterReading {
        provider_id,
        meter_address,
        total_kwh: kwh_reading,
        timestamp,
        nonce,
        signature,
    };
    match energy::submit_meter_reading(reading, block) {
        Ok(credit) => Ok(credit_value(&credit)),
        Err(err) => Err(map_energy_error(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_error_mapping_preserves_quorum_context() {
        let rpc_err = map_energy_error(EnergyMarketError::SettlementBelowQuorum {
            required_ppm: 750_000,
            actual_ppm: 250_000,
        });
        assert_eq!(rpc_err.code, ERR_QUORUM_FAILED);
        assert!(
            rpc_err.message.contains("required 750000")
                && rpc_err.message.contains("actual 250000"),
            "message should preserve quorum details: {}",
            rpc_err.message
        );
    }

    #[test]
    fn dispute_error_mapping_preserves_context() {
        let rpc_err = map_dispute_error(DisputeError::UnknownDispute { dispute_id: 42 });
        assert_eq!(rpc_err.code, ERR_SETTLEMENT_CONFLICT);
        assert!(rpc_err.message.contains("dispute 42 not found"));
    }

    #[test]
    fn signature_length_enforced() {
        let rpc_err = decode_signature("00").unwrap_err();
        assert_eq!(rpc_err.code, ERR_INVALID_PARAMS);
        assert!(rpc_err.message.contains("bytes"));
    }
}
