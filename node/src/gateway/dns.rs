use super::{mobile_cache, read_receipt};
use crate::governance::NODE_GOV_STORE;
use crate::simple_db::{names, SimpleDb};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::ERR_DNS_SIG_INVALID;
use crate::{Account, Blockchain, TokenBalance};
use concurrency::Lazy;
use crypto_suite::signatures::ed25519::{
    Signature, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH,
};
#[cfg(feature = "telemetry")]
use diagnostics::tracing::warn;
use foundation_serialization::binary_cursor::{Reader as BinaryReader, Writer as BinaryWriter};
use foundation_serialization::json::{Map, Number, Value};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    adjust_dns_stake_locked, record_dns_auction_cancelled, record_dns_auction_completed,
    update_dns_auction_status_metrics, DNS_VERIFICATION_FAIL_TOTAL, GATEWAY_DNS_LOOKUP_TOTAL,
};
use runtime::net::lookup_txt;

static DNS_DB: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_DNS_DB_PATH").unwrap_or_else(|_| "dns_db".into());
    Mutex::new(SimpleDb::open_named(names::GATEWAY_DNS, &path))
});

static ALLOW_EXTERNAL: AtomicBool = AtomicBool::new(false);
static DISABLE_VERIFY: AtomicBool = AtomicBool::new(false);
const VERIFY_TTL: Duration = Duration::from_secs(3600);
/// DNS TXT record verification prefix - external domains must have TXT record: "tb-verification={node_id}"
const DNS_VERIFICATION_PREFIX: &str = "tb-verification=";
static REHEARSAL: AtomicBool = AtomicBool::new(true);

type TxtResolver = Box<dyn Fn(&str) -> Vec<String> + Send + Sync>;
static TXT_RESOLVER: Lazy<Mutex<TxtResolver>> =
    Lazy::new(|| Mutex::new(Box::new(default_txt_resolver)));
static VERIFY_CACHE: Lazy<Mutex<HashMap<String, (bool, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static TREASURY_HOOK: Lazy<Mutex<Option<Arc<dyn Fn(u64) + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));

static LEDGER_CONTEXT: Lazy<Mutex<Option<Arc<dyn DomainLedger + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));
static LEDGER_COUNTER: AtomicU64 = AtomicU64::new(1);
static SANDBOX_LEDGER: Lazy<Arc<dyn DomainLedger + Send + Sync>> =
    Lazy::new(|| Arc::new(SandboxLedger::new()) as Arc<dyn DomainLedger + Send + Sync>);
const SANDBOX_LEDGER_LOG_CAPACITY: usize = 1024;
static SANDBOX_LEDGER_LOG: Lazy<Mutex<VecDeque<SandboxLedgerRecord>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(SANDBOX_LEDGER_LOG_CAPACITY)));
const DNS_METRIC_CAPACITY: usize = 4096;
static DNS_METRICS: Lazy<Mutex<VecDeque<DnsMetricEvent>>> =
    Lazy::new(|| Mutex::new(VecDeque::with_capacity(DNS_METRIC_CAPACITY)));

/// Dynamic reserve pricing configuration
#[derive(Debug, Clone)]
struct DynamicReservePricingConfig {
    /// Enable dynamic reserve pricing
    enabled: bool,
    /// Base reserve price in BLOCK (default 1000)
    base_reserve: u64,
    /// Length sensitivity factor (default 0.1 = 10% per character)
    length_sensitivity: f64,
    /// Historical performance weight (default 0.5)
    history_weight: f64,
}

impl Default for DynamicReservePricingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_reserve: 1000,
            length_sensitivity: 0.1,
            history_weight: 0.5,
        }
    }
}

static DYNAMIC_RESERVE_CONFIG: Lazy<Mutex<DynamicReservePricingConfig>> =
    Lazy::new(|| Mutex::new(DynamicReservePricingConfig::default()));

/// Compute dynamic reserve price based on domain quality metrics
///
/// Formula:
/// ```text
/// reserve_price = base_reserve * length_multiplier * history_multiplier
///
/// where:
///   length_multiplier = max(0.2, 1.0 - sensitivity * max(0, length - 3))
///   history_multiplier = if prior auction exists:
///                          1.0 + history_weight * ((historical_price / base_reserve) - 1.0).clamp(0.0, 2.0)
///                        else:
///                          1.0
/// ```
///
/// **Domain Quality Factors:**
/// - **Length**: Shorter domains (3-4 chars) get premium pricing
/// - **Historical performance**: Domains with successful prior auctions get premium
/// - **Floor protection**: Minimum 20% of base reserve prevents devaluation
///
fn compute_dynamic_reserve_price(domain: &str, prior_auction: Option<&DomainAuctionRecord>) -> u64 {
    let config = DYNAMIC_RESERVE_CONFIG
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    if !config.enabled {
        return config.base_reserve;
    }

    let base = config.base_reserve as f64;

    // 1. Length multiplier (shorter = more valuable)
    // 3-char domains: 1.0x, 4-char: 0.9x, 5-char: 0.8x, etc.
    // Floor at 0.2x (20% of base) for very long domains
    let length = domain.chars().count() as f64;
    let length_multiplier = (1.0 - config.length_sensitivity * (length - 3.0).max(0.0)).max(0.2);

    // 2. Historical performance multiplier
    let history_multiplier = if let Some(prior) = prior_auction {
        if prior.status == AuctionStatus::Settled {
            if let Some(winning_bid) = &prior.highest_bid {
                // Compute historical price premium
                let historical_price = winning_bid.amount as f64;
                let price_ratio = historical_price / base;
                // Apply history weight to the price premium (clamped to [0.0, 2.0])
                let premium = (price_ratio - 1.0).clamp(0.0, 2.0);
                1.0 + config.history_weight * premium
            } else {
                1.0
            }
        } else {
            1.0
        }
    } else {
        1.0
    };

    // Combine all factors
    let final_price = base * length_multiplier * history_multiplier;

    // Round and ensure minimum of 1 BLOCK
    final_price.round().max(1.0) as u64
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    amount: u64,
    stake_reference: Option<String>,
    placed_at: u64,
    stake_locked: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainAuctionRecord {
    domain: String,
    seller_account: Option<String>,
    seller_stake: Option<String>,
    protocol_fee_bps: u16,
    royalty_bps: u16,
    min_bid: u64,
    stake_requirement: u64,
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
    last_sale_price: u64,
    owner_stake: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DomainSaleRecord {
    domain: String,
    sold_at: u64,
    seller_account: Option<String>,
    buyer_account: String,
    price: u64,
    protocol_fee: u64,
    royalty_fee: u64,
    ledger_events: Vec<LedgerEventRecord>,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
enum LedgerEventKind {
    DebitBidder,
    CreditSeller,
    CreditRoyalty,
    CreditTreasury,
    RefundStake,
    StakeDeposit,
    StakeWithdraw,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct LedgerEventRecord {
    kind: LedgerEventKind,
    account: String,
    amount: u64,
    tx_ref: String,
}

#[derive(Clone)]
pub struct LedgerBatchCommand {
    kind: LedgerBatchKind,
    account: Option<String>,
    amount: u64,
    memo: String,
}

#[derive(Clone, Copy)]
pub enum LedgerBatchKind {
    Debit,
    Credit,
    CreditTreasury,
}

impl LedgerBatchCommand {
    pub fn debit(account: &str, amount: u64, memo: &str) -> Self {
        Self {
            kind: LedgerBatchKind::Debit,
            account: Some(account.to_string()),
            amount,
            memo: memo.to_string(),
        }
    }

    pub fn credit(account: &str, amount: u64, memo: &str) -> Self {
        Self {
            kind: LedgerBatchKind::Credit,
            account: Some(account.to_string()),
            amount,
            memo: memo.to_string(),
        }
    }

    pub fn credit_treasury(amount: u64, memo: &str) -> Self {
        Self {
            kind: LedgerBatchKind::CreditTreasury,
            account: None,
            amount,
            memo: memo.to_string(),
        }
    }

    fn amount(&self) -> u64 {
        self.amount
    }

    fn account_label(&self) -> Option<&str> {
        self.account.as_deref()
    }

    fn kind(&self) -> LedgerBatchKind {
        self.kind
    }

    fn memo(&self) -> &str {
        &self.memo
    }
}

pub struct LedgerBatchResult {
    pub account: String,
    pub tx_ref: String,
}

fn push_metric(kind: DnsMetricKind) {
    let mut guard = DNS_METRICS.lock().unwrap_or_else(|e| e.into_inner());
    guard.push_back(DnsMetricEvent { ts: now_ts(), kind });
    while guard.len() > DNS_METRIC_CAPACITY {
        guard.pop_front();
    }
}

fn record_txt_result(ok: bool) {
    push_metric(DnsMetricKind::TxtResult { ok });
}

fn record_auction_completed(duration_secs: u64, settlement: u64) {
    push_metric(DnsMetricKind::AuctionCompleted {
        duration_secs,
        settlement,
    });
    #[cfg(feature = "telemetry")]
    record_dns_auction_completed(duration_secs, settlement);
}

fn record_auction_cancelled() {
    push_metric(DnsMetricKind::AuctionCancelled);
    #[cfg(feature = "telemetry")]
    record_dns_auction_cancelled();
}

fn record_stake_lock(amount: u64) {
    if amount > 0 {
        push_metric(DnsMetricKind::StakeLock { _amount: amount });
        #[cfg(feature = "telemetry")]
        adjust_dns_stake_locked(i64::try_from(amount).unwrap_or(i64::MAX));
    }
}

fn record_stake_unlock(amount: u64) {
    if amount > 0 {
        push_metric(DnsMetricKind::StakeUnlock { _amount: amount });
        #[cfg(feature = "telemetry")]
        adjust_dns_stake_locked(-i64::try_from(amount).unwrap_or(i64::MAX));
    }
}

fn percentile_from_sorted(sorted: &[u64], quantile: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let clamped = quantile.clamp(0.0, 1.0);
    let idx = ((sorted.len() as f64 - 1.0) * clamped)
        .round()
        .clamp(0.0, (sorted.len() - 1) as f64) as usize;
    sorted[idx]
}

fn stats_value(samples: &[u64]) -> Value {
    if samples.is_empty() {
        return Value::Null;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    json_map(vec![
        (
            "p50",
            Value::Number(Number::from(percentile_from_sorted(&sorted, 0.50))),
        ),
        (
            "p90",
            Value::Number(Number::from(percentile_from_sorted(&sorted, 0.90))),
        ),
        (
            "max",
            Value::Number(Number::from(*sorted.last().unwrap_or(&0))),
        ),
        ("samples", Value::Number(Number::from(sorted.len() as u64))),
    ])
}

fn value_or_null(value: Option<u64>) -> Value {
    value
        .map(|v| Value::Number(Number::from(v)))
        .unwrap_or(Value::Null)
}

fn ppm_ratio(numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        0
    } else {
        numerator
            .saturating_mul(1_000_000)
            .checked_div(denominator)
            .unwrap_or(1_000_000)
            .min(1_000_000)
    }
}

pub fn governance_metrics_snapshot(window_secs: u64) -> DnsMetricsSnapshot {
    let cutoff = now_ts().saturating_sub(window_secs.max(1));
    let mut snapshot = DnsMetricsSnapshot::default();
    let guard = DNS_METRICS.lock().unwrap_or_else(|e| e.into_inner());
    for event in guard.iter().rev() {
        if event.ts < cutoff {
            break;
        }
        match &event.kind {
            DnsMetricKind::TxtResult { ok } => {
                snapshot.txt_attempts = snapshot.txt_attempts.saturating_add(1);
                if *ok {
                    snapshot.txt_successes = snapshot.txt_successes.saturating_add(1);
                }
            }
            DnsMetricKind::AuctionCompleted {
                duration_secs,
                settlement,
            } => {
                snapshot.auction_completions = snapshot.auction_completions.saturating_add(1);
                snapshot.settle_durations_secs.push(*duration_secs);
                snapshot.settlement_amounts.push(*settlement);
            }
            DnsMetricKind::AuctionCancelled => {
                snapshot.auction_cancels = snapshot.auction_cancels.saturating_add(1);
            }
            DnsMetricKind::StakeUnlock { .. } => {
                snapshot.stake_unlock_events = snapshot.stake_unlock_events.saturating_add(1);
            }
            DnsMetricKind::StakeLock { .. } => {}
        }
    }
    snapshot
}

pub fn total_locked_stake() -> u64 {
    let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let mut total = 0u64;
    for key in db.keys_with_prefix("dns_stake/") {
        if let Some(bytes) = db.get(&key) {
            if let Ok(record) = decode_stake(&bytes) {
                total = total.saturating_add(record.locked);
            }
        }
    }
    total
}

#[derive(Clone, Debug)]
struct DnsMetricEvent {
    ts: u64,
    kind: DnsMetricKind,
}

#[derive(Clone, Debug)]
enum DnsMetricKind {
    TxtResult { ok: bool },
    AuctionCompleted { duration_secs: u64, settlement: u64 },
    AuctionCancelled,
    StakeLock { _amount: u64 },
    StakeUnlock { _amount: u64 },
}

#[derive(Clone, Debug, Default)]
pub struct DnsMetricsSnapshot {
    pub txt_attempts: u64,
    pub txt_successes: u64,
    pub auction_completions: u64,
    pub auction_cancels: u64,
    pub settle_durations_secs: Vec<u64>,
    pub settlement_amounts: Vec<u64>,
    pub stake_unlock_events: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct StakeEscrowRecord {
    reference: String,
    owner_account: String,
    amount: u64,
    locked: u64,
    ledger_events: Vec<LedgerEventRecord>,
}

impl StakeEscrowRecord {
    fn available(&self) -> u64 {
        self.amount.saturating_sub(self.locked)
    }

    fn lock(&mut self, amount: u64) -> Result<(), AuctionError> {
        if self.available() < amount {
            return Err(AuctionError::BidInsufficientStake);
        }
        self.locked = self.locked.saturating_add(amount);
        Ok(())
    }

    fn unlock(&mut self, amount: u64) -> u64 {
        let unlock_amount = amount.min(self.locked);
        self.locked = self.locked.saturating_sub(unlock_amount);
        unlock_amount
    }

    fn push_event(&mut self, kind: LedgerEventKind, amount: u64, tx_ref: String) {
        self.ledger_events.push(LedgerEventRecord {
            kind,
            account: self.owner_account.clone(),
            amount: amount,
            tx_ref,
        });
    }
}

#[derive(Clone)]
pub struct BlockchainLedger {
    chain: Arc<Mutex<Blockchain>>,
    treasury_account: String,
}

impl BlockchainLedger {
    pub fn new(chain: Arc<Mutex<Blockchain>>, treasury_account: String) -> Self {
        Self {
            chain,
            treasury_account,
        }
    }

    fn blank_account(address: &str) -> Account {
        Account {
            address: address.to_string(),
            balance: TokenBalance { amount: 0 },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        }
    }
}

impl DomainLedger for BlockchainLedger {
    fn apply_batch(
        &self,
        commands: &[LedgerBatchCommand],
    ) -> Result<Vec<LedgerBatchResult>, AuctionError> {
        let mut guard = self.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut staged: HashMap<String, Account> = HashMap::new();

        for command in commands {
            let target_account = match command.kind() {
                LedgerBatchKind::CreditTreasury => self.treasury_account.clone(),
                _ => command
                    .account_label()
                    .map(|s| s.to_string())
                    .ok_or(AuctionError::InvalidBidder)?,
            };

            if matches!(command.kind(), LedgerBatchKind::Debit)
                && !staged.contains_key(&target_account)
                && !guard.accounts.contains_key(&target_account)
            {
                return Err(AuctionError::BidInsufficientStake);
            }

            let entry = staged.entry(target_account.clone()).or_insert_with(|| {
                guard
                    .accounts
                    .get(&target_account)
                    .cloned()
                    .unwrap_or_else(|| Self::blank_account(&target_account))
            });

            match command.kind() {
                LedgerBatchKind::Debit => {
                    if entry.balance.amount < command.amount() {
                        return Err(AuctionError::BidInsufficientStake);
                    }
                    entry.balance.amount -= command.amount();
                }
                LedgerBatchKind::Credit => {
                    entry.balance.amount = entry.balance.amount.saturating_add(command.amount());
                }
                LedgerBatchKind::CreditTreasury => {
                    entry.balance.amount = entry.balance.amount.saturating_add(command.amount());
                }
            }
            let _ = command.memo();
        }

        for (account, updated) in staged.iter() {
            guard.accounts.insert(account.clone(), updated.clone());
        }

        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            let (account, prefix) = match command.kind() {
                LedgerBatchKind::Debit => (
                    command
                        .account_label()
                        .map(|s| s.to_string())
                        .ok_or(AuctionError::InvalidBidder)?,
                    "dns-debit",
                ),
                LedgerBatchKind::Credit => (
                    command
                        .account_label()
                        .map(|s| s.to_string())
                        .ok_or(AuctionError::InvalidBidder)?,
                    "dns-credit",
                ),
                LedgerBatchKind::CreditTreasury => (self.treasury_account.clone(), "dns-treasury"),
            };
            let _ = command.memo();
            results.push(LedgerBatchResult {
                account,
                tx_ref: next_ledger_ref(prefix),
            });
        }

        Ok(results)
    }
}

#[derive(Default)]
struct SandboxLedger {
    seq: AtomicU64,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct SandboxCommandRecord {
    kind: String,
    account: Option<String>,
    amount: u64,
    memo: String,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct SandboxLedgerRecord {
    ts: u64,
    commands: Vec<SandboxCommandRecord>,
}

impl SandboxLedger {
    fn new() -> Self {
        Self {
            seq: AtomicU64::new(0),
        }
    }

    fn record(&self, commands: &[LedgerBatchCommand]) {
        if commands.is_empty() {
            return;
        }
        let _ = self.seq.fetch_add(1, Ordering::SeqCst);
        let entries: Vec<SandboxCommandRecord> = commands
            .iter()
            .map(|cmd| SandboxCommandRecord {
                kind: match cmd.kind() {
                    LedgerBatchKind::Debit => "debit".into(),
                    LedgerBatchKind::Credit => "credit".into(),
                    LedgerBatchKind::CreditTreasury => "credit_treasury".into(),
                },
                account: cmd.account_label().map(|s| s.to_string()),
                amount: cmd.amount(),
                memo: cmd.memo().to_string(),
            })
            .collect();
        let record = SandboxLedgerRecord {
            ts: now_ts(),
            commands: entries,
        };
        let mut log = SANDBOX_LEDGER_LOG.lock().unwrap_or_else(|e| e.into_inner());
        if log.len() >= SANDBOX_LEDGER_LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(record);
    }
}

impl DomainLedger for SandboxLedger {
    fn apply_batch(
        &self,
        commands: &[LedgerBatchCommand],
    ) -> Result<Vec<LedgerBatchResult>, AuctionError> {
        self.record(commands);
        let ts = now_ts();
        let mut results = Vec::with_capacity(commands.len());
        for (idx, command) in commands.iter().enumerate() {
            let account = command
                .account_label()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "sandbox".into());
            results.push(LedgerBatchResult {
                account,
                tx_ref: format!("sandbox-{ts}-{idx}"),
            });
        }
        Ok(results)
    }
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
    InvalidSeller,
    InvalidStakeReference,
    StakeAmountZero,
    StakeLocked,
    StakeMissing,
    StakeOwnerMismatch,
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
            AuctionError::InvalidSeller => -32072,
            AuctionError::InvalidStakeReference => -32073,
            AuctionError::StakeAmountZero => -32074,
            AuctionError::StakeLocked => -32075,
            AuctionError::StakeMissing => -32076,
            AuctionError::StakeOwnerMismatch => -32077,
            AuctionError::NoBids => -32078,
            AuctionError::Storage => -32079,
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
            AuctionError::InvalidSeller => "invalid seller account",
            AuctionError::InvalidStakeReference => "invalid stake reference",
            AuctionError::StakeAmountZero => "stake deposit must be greater than zero",
            AuctionError::StakeLocked => "stake remains locked",
            AuctionError::StakeMissing => "stake reference not found",
            AuctionError::StakeOwnerMismatch => "stake owner mismatch",
            AuctionError::NoBids => "auction has no bids",
            AuctionError::Storage => "auction storage error",
        }
    }
}

pub trait DomainLedger: Send + Sync {
    fn apply_batch(
        &self,
        commands: &[LedgerBatchCommand],
    ) -> Result<Vec<LedgerBatchResult>, AuctionError>;

    fn debit(&self, account: &str, amount: u64, memo: &str) -> Result<String, AuctionError> {
        let commands = [LedgerBatchCommand::debit(account, amount, memo)];
        let mut results = self.apply_batch(&commands)?;
        Ok(results.pop().map(|r| r.tx_ref).unwrap_or_default())
    }

    fn credit(&self, account: &str, amount: u64, memo: &str) -> Result<String, AuctionError> {
        let commands = [LedgerBatchCommand::credit(account, amount, memo)];
        let mut results = self.apply_batch(&commands)?;
        Ok(results.pop().map(|r| r.tx_ref).unwrap_or_default())
    }

    fn credit_treasury(&self, amount: u64, memo: &str) -> Result<(String, String), AuctionError> {
        let commands = [LedgerBatchCommand::credit_treasury(amount, memo)];
        let mut results = self.apply_batch(&commands)?;
        let result = results.pop().unwrap_or(LedgerBatchResult {
            account: String::new(),
            tx_ref: String::new(),
        });
        Ok((result.account, result.tx_ref))
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

fn stake_key(reference: &str) -> String {
    format!("dns_stake/{reference}")
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

fn decode_stake(bytes: &[u8]) -> Result<StakeEscrowRecord, AuctionError> {
    let mut reader = BinaryReader::new(bytes);
    let record = read_stake(&mut reader).map_err(map_decode_error)?;
    ensure_exhausted(&reader).map_err(map_decode_error)?;
    Ok(record)
}

fn encode_stake(record: &StakeEscrowRecord) -> Result<Vec<u8>, AuctionError> {
    let mut writer = BinaryWriter::new();
    write_stake(&mut writer, record);
    Ok(writer.finish())
}

fn load_stake(db: &SimpleDb, reference: &str) -> Result<Option<StakeEscrowRecord>, AuctionError> {
    db.get(&stake_key(reference))
        .map(|bytes| decode_stake(&bytes))
        .transpose()
}

fn load_stake_or_err(db: &SimpleDb, reference: &str) -> Result<StakeEscrowRecord, AuctionError> {
    load_stake(db, reference)?.ok_or(AuctionError::BidInsufficientStake)
}

fn persist_stake(db: &mut SimpleDb, record: &StakeEscrowRecord) -> Result<(), AuctionError> {
    let bytes = encode_stake(record)?;
    db.insert(&stake_key(&record.reference), bytes);
    Ok(())
}

fn record_treasury_fee(amount: u64) {
    if amount == 0 {
        return;
    }
    if let Some(hook) = TREASURY_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .cloned()
    {
        hook(amount);
        return;
    }
    if let Err(err) = NODE_GOV_STORE.record_treasury_accrual(amount) {
        #[cfg(feature = "telemetry")]
        warn!(
            amount,
            ?err,
            "failed to accrue treasury fee from dns auction"
        );
        #[cfg(not(feature = "telemetry"))]
        let _ = (amount, err);
    }
}

fn ledger_handle() -> Result<Arc<dyn DomainLedger + Send + Sync>, AuctionError> {
    if REHEARSAL.load(Ordering::Relaxed) {
        return Ok(SANDBOX_LEDGER.clone());
    }
    LEDGER_CONTEXT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .cloned()
        .ok_or(AuctionError::Storage)
}

fn next_ledger_ref(prefix: &str) -> String {
    let seq = LEDGER_COUNTER.fetch_add(1, Ordering::SeqCst);
    let ts = now_ts();
    format!("{prefix}-{ts}-{seq}")
}

fn apply_ledger_plan(
    ledger: &dyn DomainLedger,
    plan: Vec<(LedgerEventKind, LedgerBatchCommand)>,
) -> Result<Vec<LedgerEventRecord>, AuctionError> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let commands: Vec<LedgerBatchCommand> = plan.iter().map(|(_, cmd)| cmd.clone()).collect();
    let results = ledger.apply_batch(&commands)?;
    Ok(plan
        .into_iter()
        .zip(results.into_iter())
        .map(|((kind, command), result)| LedgerEventRecord {
            kind,
            account: result.account,
            amount: command.amount(),
            tx_ref: result.tx_ref,
        })
        .collect())
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn install_treasury_hook<F>(hook: F)
where
    F: Fn(u64) + Send + Sync + 'static,
{
    *TREASURY_HOOK.lock().unwrap() = Some(Arc::new(hook));
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn clear_treasury_hook() {
    TREASURY_HOOK.lock().unwrap().take();
}

pub fn install_ledger_context(ctx: Arc<dyn DomainLedger + Send + Sync>) {
    *LEDGER_CONTEXT.lock().unwrap() = Some(ctx);
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn clear_ledger_context() {
    LEDGER_CONTEXT.lock().unwrap().take();
}

pub fn set_rehearsal(enabled: bool) {
    REHEARSAL.store(enabled, Ordering::Relaxed);
}

pub fn rehearsal_enabled() -> bool {
    REHEARSAL.load(Ordering::Relaxed)
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn seed_stake(reference: &str, owner: &str, amount: u64) {
    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let record = StakeEscrowRecord {
        reference: reference.to_string(),
        owner_account: owner.to_string(),
        amount,
        locked: 0,
        ledger_events: Vec::new(),
    };
    persist_stake(&mut db, &record).expect("seed stake");
}

#[cfg(any(test, feature = "integration-tests"))]
pub struct StakeSnapshot {
    pub owner_account: String,
    pub amount: u64,
    pub locked: u64,
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn stake_snapshot(reference: &str) -> Option<StakeSnapshot> {
    let record = {
        let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        load_stake(&db, reference).ok().flatten()?
    }; // DNS_DB lock automatically released here

    Some(StakeSnapshot {
        owner_account: record.owner_account,
        amount: record.amount,
        locked: record.locked,
    })
}

pub fn register_stake(params: &Value) -> Result<Value, AuctionError> {
    let reference = params
        .get("reference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if reference.is_empty() {
        return Err(AuctionError::InvalidStakeReference);
    }
    let owner = params
        .get("owner_account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if owner.is_empty() {
        return Err(AuctionError::InvalidBidder);
    }
    let deposit = params.get("deposit").and_then(|v| v.as_u64()).unwrap_or(0);
    if deposit == 0 {
        return Err(AuctionError::StakeAmountZero);
    }

    let ledger = ledger_handle()?;

    // Perform database operations with lock held, then release before building return value
    let (tx_ref, updated) = {
        let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let existing = load_stake(&db, reference)?;
        let record = match existing {
            Some(record) => {
                if record.owner_account != owner {
                    return Err(AuctionError::StakeOwnerMismatch);
                }
                record
            }
            None => StakeEscrowRecord {
                reference: reference.to_string(),
                owner_account: owner.to_string(),
                amount: 0,
                locked: 0,
                ledger_events: Vec::new(),
            },
        };

        let mut updated = record.clone();
        let memo = format!("dns_stake_deposit:{reference}");
        let tx_ref = ledger.debit(owner, deposit, &memo)?;
        updated.amount = updated.amount.saturating_add(deposit);
        updated.push_event(LedgerEventKind::StakeDeposit, deposit, tx_ref.clone());

        if let Err(err) = persist_stake(&mut db, &updated) {
            let revert_memo = format!("dns_stake_revert:{reference}");
            let _ = ledger.credit(owner, deposit, &revert_memo);
            return Err(err);
        }

        (tx_ref, updated)
    }; // DNS_DB lock automatically released here

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("tx_ref", Value::String(tx_ref)),
        ("stake", stake_to_json(&updated)),
    ]))
}

pub fn withdraw_stake(params: &Value) -> Result<Value, AuctionError> {
    let reference = params
        .get("reference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if reference.is_empty() {
        return Err(AuctionError::InvalidStakeReference);
    }
    let owner = params
        .get("owner_account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if owner.is_empty() {
        return Err(AuctionError::InvalidBidder);
    }
    let withdraw = params.get("withdraw").and_then(|v| v.as_u64()).unwrap_or(0);
    if withdraw == 0 {
        return Err(AuctionError::StakeAmountZero);
    }

    let ledger = ledger_handle()?;

    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let record = load_stake_or_err(&db, reference)?;
    if record.owner_account != owner {
        return Err(AuctionError::StakeOwnerMismatch);
    }
    if withdraw > record.available() {
        return Err(AuctionError::StakeLocked);
    }

    let memo = format!("dns_stake_withdraw:{reference}");
    let mut updated = record.clone();
    updated.amount = updated.amount.saturating_sub(withdraw);

    match ledger.credit(owner, withdraw, &memo) {
        Ok(tx_ref) => {
            updated.push_event(LedgerEventKind::StakeWithdraw, withdraw, tx_ref.clone());
            if let Err(err) = persist_stake(&mut db, &updated) {
                let revert_memo = format!("dns_stake_revert:{reference}");
                let _ = ledger.debit(owner, withdraw, &revert_memo);
                let _ = persist_stake(&mut db, &record);
                return Err(err);
            }
            record_stake_unlock(withdraw);
            Ok(json_map(vec![
                ("status", Value::String("ok".to_string())),
                ("withdrawn", Value::Number(Number::from(withdraw))),
                ("tx_ref", Value::String(tx_ref)),
                ("stake", stake_to_json(&updated)),
            ]))
        }
        Err(err) => Err(err),
    }
}

pub fn stake_status(params: &Value) -> Result<Value, AuctionError> {
    let reference = params
        .get("reference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if reference.is_empty() {
        return Err(AuctionError::InvalidStakeReference);
    }
    let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let record = load_stake(&db, reference)?;
    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        (
            "stake",
            record.as_ref().map(stake_to_json).unwrap_or(Value::Null),
        ),
    ]))
}

pub fn cancel_sale(params: &Value) -> Result<Value, AuctionError> {
    let domain = params
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if domain.is_empty() {
        return Err(AuctionError::InvalidDomain);
    }
    let seller = params
        .get("seller_account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if seller.is_empty() {
        return Err(AuctionError::InvalidSeller);
    }

    // Perform all database operations with lock held, then release before building return value
    let record = {
        let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let key = auction_key(domain);
        let mut record = match db.get(&key) {
            Some(bytes) => decode_auction(&bytes)?,
            None => return Err(AuctionError::AuctionMissing),
        };

        if record.status != AuctionStatus::Active {
            return Err(AuctionError::AuctionClosed);
        }
        if record.seller_account.as_ref().map(|s| s.as_str()) != Some(seller) {
            return Err(AuctionError::OwnershipMismatch);
        }

        if let Some(mut highest) = record.highest_bid.take() {
            if let (Some(reference), amount) =
                (highest.stake_reference.as_ref(), highest.stake_locked)
            {
                if amount > 0 {
                    let mut stake_record = load_stake_or_err(&db, reference)?;
                    stake_record.unlock(amount);
                    persist_stake(&mut db, &stake_record)?;
                }
            }
            highest.stake_locked = 0;
            record.highest_bid = None;
            for bid in record.bids.iter_mut().rev() {
                if bid.bidder == highest.bidder && bid.placed_at == highest.placed_at {
                    bid.stake_locked = 0;
                    break;
                }
            }
        }

        if let Some(reference) = record.seller_stake.as_ref() {
            if !reference.is_empty() {
                if let Some(mut stake_record) = load_stake(&db, reference)? {
                    if stake_record.owner_account == seller && stake_record.locked > 0 {
                        let locked = stake_record.locked;
                        stake_record.unlock(locked);
                        persist_stake(&mut db, &stake_record)?;
                    }
                }
            }
        }

        record.status = AuctionStatus::Cancelled;
        record.end_ts = now_ts();
        let bytes = encode_auction(&record)?;
        db.insert(&key, bytes);
        record_auction_cancelled();

        record
    }; // DNS_DB lock automatically released here

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("auction", auction_to_json(&record)),
    ]))
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
        ("amount", Value::Number(Number::from(bid.amount))),
        ("placed_at", Value::Number(Number::from(bid.placed_at))),
        (
            "stake_reference",
            bid.stake_reference
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
        (
            "stake_locked",
            Value::Number(Number::from(bid.stake_locked)),
        ),
    ])
}

fn stake_to_json(record: &StakeEscrowRecord) -> Value {
    let events = Value::Array(
        record
            .ledger_events
            .iter()
            .map(ledger_event_to_json)
            .collect(),
    );
    json_map(vec![
        ("reference", Value::String(record.reference.clone())),
        ("owner_account", Value::String(record.owner_account.clone())),
        ("amount", Value::Number(Number::from(record.amount))),
        ("locked", Value::Number(Number::from(record.locked))),
        ("available", Value::Number(Number::from(record.available()))),
        ("ledger_events", events),
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
        ("min_bid", Value::Number(Number::from(record.min_bid))),
        (
            "stake_requirement",
            Value::Number(Number::from(record.stake_requirement)),
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
            "last_sale_price",
            Value::Number(Number::from(record.last_sale_price)),
        ),
        (
            "owner_stake",
            record
                .owner_stake
                .as_ref()
                .map(|s| Value::String(s.clone()))
                .unwrap_or(Value::Null),
        ),
    ])
}

fn sale_to_json(record: &DomainSaleRecord) -> Value {
    let events = Value::Array(
        record
            .ledger_events
            .iter()
            .map(ledger_event_to_json)
            .collect(),
    );
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
        ("price", Value::Number(Number::from(record.price))),
        (
            "protocol_fee",
            Value::Number(Number::from(record.protocol_fee)),
        ),
        (
            "royalty_fee",
            Value::Number(Number::from(record.royalty_fee)),
        ),
        ("ledger_events", events),
    ])
}

fn ledger_event_to_json(event: &LedgerEventRecord) -> Value {
    let kind = match event.kind {
        LedgerEventKind::DebitBidder => "debit_bidder",
        LedgerEventKind::CreditSeller => "credit_seller",
        LedgerEventKind::CreditRoyalty => "credit_royalty",
        LedgerEventKind::CreditTreasury => "credit_treasury",
        LedgerEventKind::RefundStake => "refund_stake",
        LedgerEventKind::StakeDeposit => "stake_deposit",
        LedgerEventKind::StakeWithdraw => "stake_withdraw",
    };
    json_map(vec![
        ("kind", Value::String(kind.to_string())),
        ("account", Value::String(event.account.clone())),
        ("amount", Value::Number(Number::from(event.amount))),
        ("tx_ref", Value::String(event.tx_ref.clone())),
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

const BID_FIELD_COUNT: Option<u64> = None;
const AUCTION_FIELD_COUNT: Option<u64> = None;
const OWNERSHIP_FIELD_COUNT: Option<u64> = None;
const SALE_FIELD_COUNT: Option<u64> = None;

fn write_bid(writer: &mut BinaryWriter, bid: &DomainBidRecord) {
    writer.write_struct(|s| {
        s.field_string("bidder", &bid.bidder);
        s.field_u64("amount", bid.amount);
        s.field_option_string("stake_reference", bid.stake_reference.as_deref());
        s.field_u64("placed_at", bid.placed_at);
        s.field_u64("stake_locked", bid.stake_locked);
    });
}

fn read_bid(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainBidRecord> {
    let mut bidder = None;
    let mut amount = None;
    let mut stake: Option<Option<String>> = None;
    let mut placed_at = None;
    let mut stake_locked = None;

    decode_struct(reader, BID_FIELD_COUNT, |key, reader| match key {
        "bidder" => {
            let value = reader.read_string()?;
            assign_once(&mut bidder, value, "bidder")
        }
        "amount" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount")
        }
        "stake_reference" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut stake, value, "stake_reference")
        }
        "placed_at" => {
            let value = reader.read_u64()?;
            assign_once(&mut placed_at, value, "placed_at")
        }
        "stake_locked" => {
            let value = reader.read_u64()?;
            assign_once(&mut stake_locked, value, "stake_locked")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainBidRecord {
        bidder: bidder.ok_or(DecodeError::MissingField("bidder"))?,
        amount: amount.ok_or(DecodeError::MissingField("amount"))?,
        stake_reference: stake.unwrap_or(None),
        placed_at: placed_at.ok_or(DecodeError::MissingField("placed_at"))?,
        stake_locked: stake_locked.unwrap_or(0),
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

fn ledger_event_kind_to_u8(kind: LedgerEventKind) -> u8 {
    match kind {
        LedgerEventKind::DebitBidder => 0,
        LedgerEventKind::CreditSeller => 1,
        LedgerEventKind::CreditRoyalty => 2,
        LedgerEventKind::CreditTreasury => 3,
        LedgerEventKind::RefundStake => 4,
        LedgerEventKind::StakeDeposit => 5,
        LedgerEventKind::StakeWithdraw => 6,
    }
}

fn ledger_event_kind_from_u8(value: u8) -> binary_struct::Result<LedgerEventKind> {
    match value {
        0 => Ok(LedgerEventKind::DebitBidder),
        1 => Ok(LedgerEventKind::CreditSeller),
        2 => Ok(LedgerEventKind::CreditRoyalty),
        3 => Ok(LedgerEventKind::CreditTreasury),
        4 => Ok(LedgerEventKind::RefundStake),
        5 => Ok(LedgerEventKind::StakeDeposit),
        6 => Ok(LedgerEventKind::StakeWithdraw),
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "LedgerEventKind",
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
        s.field_u64("min_bid", record.min_bid);
        s.field_u64("stake_requirement", record.stake_requirement);
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
    let mut min_bid = None;
    let mut stake_requirement = None;
    let mut start_ts = None;
    let mut end_ts = None;
    let mut status = None;
    let mut highest_bid: Option<Option<DomainBidRecord>> = None;
    let mut bids: Option<Vec<DomainBidRecord>> = None;

    decode_struct(reader, AUCTION_FIELD_COUNT, |key, reader| match key {
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
        "min_bid" => {
            let value = reader.read_u64()?;
            assign_once(&mut min_bid, value, "min_bid")
        }
        "stake_requirement" => {
            let value = reader.read_u64()?;
            assign_once(&mut stake_requirement, value, "stake_requirement")
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
        min_bid: min_bid.ok_or(DecodeError::MissingField("min_bid"))?,
        stake_requirement: stake_requirement
            .ok_or(DecodeError::MissingField("stake_requirement"))?,
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
        s.field_u64("last_sale_price", record.last_sale_price);
        s.field_option_string("owner_stake", record.owner_stake.as_deref());
    });
}

fn read_ownership(reader: &mut BinaryReader<'_>) -> binary_struct::Result<DomainOwnershipRecord> {
    let mut domain = None;
    let mut owner_account = None;
    let mut acquired_at = None;
    let mut royalty_bps = None;
    let mut last_sale_price = None;
    let mut owner_stake = None;

    decode_struct(reader, OWNERSHIP_FIELD_COUNT, |key, reader| match key {
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
        "last_sale_price" => {
            let value = reader.read_u64()?;
            assign_once(&mut last_sale_price, value, "last_sale_price")
        }
        "owner_stake" => {
            let value = reader.read_option_with(|r| r.read_string())?;
            assign_once(&mut owner_stake, value, "owner_stake")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainOwnershipRecord {
        domain: domain.ok_or(DecodeError::MissingField("domain"))?,
        owner_account: owner_account.ok_or(DecodeError::MissingField("owner_account"))?,
        acquired_at: acquired_at.ok_or(DecodeError::MissingField("acquired_at"))?,
        royalty_bps: royalty_bps.ok_or(DecodeError::MissingField("royalty_bps"))?,
        last_sale_price: last_sale_price.ok_or(DecodeError::MissingField("last_sale_price"))?,
        owner_stake: owner_stake.unwrap_or(None),
    })
}

fn write_sale(writer: &mut BinaryWriter, record: &DomainSaleRecord) {
    writer.write_struct(|s| {
        s.field_string("domain", &record.domain);
        s.field_option_string("seller_account", record.seller_account.as_deref());
        s.field_string("buyer_account", &record.buyer_account);
        s.field_u64("sold_at", record.sold_at);
        s.field_u64("price", record.price);
        s.field_u64("protocol_fee", record.protocol_fee);
        s.field_u64("royalty_fee", record.royalty_fee);
        s.field_with("ledger_events", |w| {
            w.write_vec_with(&record.ledger_events, write_ledger_event)
        });
    });
}

fn write_stake(writer: &mut BinaryWriter, record: &StakeEscrowRecord) {
    writer.write_struct(|s| {
        s.field_string("reference", &record.reference);
        s.field_string("owner_account", &record.owner_account);
        s.field_u64("amount", record.amount);
        s.field_u64("locked", record.locked);
        s.field_with("ledger_events", |w| {
            w.write_vec_with(&record.ledger_events, write_ledger_event)
        });
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
    let mut ledger_events: Option<Vec<LedgerEventRecord>> = None;

    decode_struct(reader, SALE_FIELD_COUNT, |key, reader| match key {
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
        "price" => {
            let value = reader.read_u64()?;
            assign_once(&mut price, value, "price")
        }
        "protocol_fee" => {
            let value = reader.read_u64()?;
            assign_once(&mut protocol_fee, value, "protocol_fee")
        }
        "royalty_fee" => {
            let value = reader.read_u64()?;
            assign_once(&mut royalty_fee, value, "royalty_fee")
        }
        "ledger_events" => {
            let value = reader.read_vec_with(|r| read_ledger_event(r))?;
            assign_once(&mut ledger_events, value, "ledger_events")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(DomainSaleRecord {
        domain: domain.ok_or(DecodeError::MissingField("domain"))?,
        seller_account: seller_account.unwrap_or(None),
        buyer_account: buyer_account.ok_or(DecodeError::MissingField("buyer_account"))?,
        sold_at: sold_at.ok_or(DecodeError::MissingField("sold_at"))?,
        price: price.ok_or(DecodeError::MissingField("price"))?,
        protocol_fee: protocol_fee.ok_or(DecodeError::MissingField("protocol_fee"))?,
        royalty_fee: royalty_fee.ok_or(DecodeError::MissingField("royalty_fee"))?,
        ledger_events: ledger_events.unwrap_or_default(),
    })
}

fn read_stake(reader: &mut BinaryReader<'_>) -> binary_struct::Result<StakeEscrowRecord> {
    let mut reference = None;
    let mut owner = None;
    let mut amount = None;
    let mut locked = None;
    let mut ledger_events: Option<Vec<LedgerEventRecord>> = None;

    decode_struct(reader, None, |key, reader| match key {
        "reference" => {
            let value = reader.read_string()?;
            assign_once(&mut reference, value, "reference")
        }
        "owner_account" => {
            let value = reader.read_string()?;
            assign_once(&mut owner, value, "owner_account")
        }
        "amount" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount")
        }
        "locked" => {
            let value = reader.read_u64()?;
            assign_once(&mut locked, value, "locked")
        }
        "ledger_events" => {
            let value = reader.read_vec_with(|r| read_ledger_event(r))?;
            assign_once(&mut ledger_events, value, "ledger_events")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(StakeEscrowRecord {
        reference: reference.ok_or(DecodeError::MissingField("reference"))?,
        owner_account: owner.ok_or(DecodeError::MissingField("owner_account"))?,
        amount: amount.ok_or(DecodeError::MissingField("amount"))?,
        locked: locked.unwrap_or(0),
        ledger_events: ledger_events.unwrap_or_default(),
    })
}

fn write_ledger_event(writer: &mut BinaryWriter, event: &LedgerEventRecord) {
    writer.write_struct(|s| {
        s.field_with("kind", |w| w.write_u8(ledger_event_kind_to_u8(event.kind)));
        s.field_string("account", &event.account);
        s.field_u64("amount", event.amount);
        s.field_string("tx_ref", &event.tx_ref);
    });
}

fn read_ledger_event(reader: &mut BinaryReader<'_>) -> binary_struct::Result<LedgerEventRecord> {
    let mut kind = None;
    let mut account = None;
    let mut amount = None;
    let mut tx_ref = None;

    decode_struct(reader, None, |key, reader| match key {
        "kind" => {
            let value = reader.read_u8()?;
            assign_once(&mut kind, ledger_event_kind_from_u8(value)?, "kind")
        }
        "account" => {
            let value = reader.read_string()?;
            assign_once(&mut account, value, "account")
        }
        "amount" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount")
        }
        "tx_ref" => {
            let value = reader.read_string()?;
            assign_once(&mut tx_ref, value, "tx_ref")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(LedgerEventRecord {
        kind: kind.ok_or(DecodeError::MissingField("kind"))?,
        account: account.ok_or(DecodeError::MissingField("account"))?,
        amount: amount.ok_or(DecodeError::MissingField("amount"))?,
        tx_ref: tx_ref.ok_or(DecodeError::MissingField("tx_ref"))?,
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

    // Perform all database operations with lock held, then release before building return value
    let record = {
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

        // Compute dynamic reserve price based on domain quality
        let computed_reserve = compute_dynamic_reserve_price(domain, prior_record.as_ref());

        // Use user-provided min_bid if specified, otherwise use computed reserve
        let min_bid = params
            .get("min_bid")
            .and_then(|v| v.as_u64())
            .unwrap_or(computed_reserve);

        if min_bid == 0 {
            return Err(AuctionError::BidTooLow);
        }
        let mut stake_requirement = params
            .get("stake_requirement")
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
        let mut seller_stake = params
            .get("seller_stake")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Database and prior_record already loaded above for dynamic pricing

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
            match (owner.owner_stake.as_ref(), seller_stake.as_ref()) {
                (Some(existing), Some(provided)) if existing != provided => {
                    return Err(AuctionError::OwnershipMismatch);
                }
                (Some(existing), None) => {
                    seller_stake = Some(existing.clone());
                }
                _ => {}
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
            min_bid: min_bid,
            stake_requirement: stake_requirement,
            start_ts,
            end_ts,
            status: AuctionStatus::Active,
            highest_bid: None,
            bids: Vec::new(),
        };

        let bytes = encode_auction(&record)?;
        db.insert(&auction_key(domain), bytes);

        record
    }; // DNS_DB lock automatically released here

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
        .get("bid")
        .and_then(|v| v.as_u64())
        .ok_or(AuctionError::BidTooLow)?;
    if amount == 0 {
        return Err(AuctionError::BidTooLow);
    }
    let stake_reference = params
        .get("stake_reference")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Perform all database operations with lock held, then release before building return value
    let record = {
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
        if amount < record.min_bid {
            return Err(AuctionError::BidTooLow);
        }
        if amount < record.stake_requirement {
            return Err(AuctionError::BidInsufficientStake);
        }
        if let Some(highest) = record.highest_bid.as_ref() {
            if amount <= highest.amount {
                return Err(AuctionError::BidTooLow);
            }
        }

        let prev_highest = record.highest_bid.clone();

        let mut locked_amount = 0u64;
        let mut stake_ref_for_bid = stake_reference.clone();

        if record.stake_requirement > 0 {
            let reference = stake_reference
                .clone()
                .ok_or(AuctionError::BidInsufficientStake)?;
            let reuse_lock = prev_highest.as_ref().map_or(false, |prev| {
                prev.bidder == bidder && prev.stake_reference.as_ref() == Some(&reference)
            });
            if reuse_lock {
                locked_amount = prev_highest
                    .as_ref()
                    .map(|prev| prev.stake_locked)
                    .unwrap_or(record.stake_requirement);
            } else {
                let mut stake_record = load_stake_or_err(&db, &reference)?;
                if stake_record.owner_account != bidder {
                    return Err(AuctionError::BidInsufficientStake);
                }
                stake_record.lock(record.stake_requirement)?;
                record_stake_lock(record.stake_requirement);
                locked_amount = record.stake_requirement;
                persist_stake(&mut db, &stake_record)?;
            }
            stake_ref_for_bid = Some(reference);
        }

        let bid = DomainBidRecord {
            bidder: bidder.to_string(),
            amount: amount,
            stake_reference: stake_ref_for_bid.clone(),
            placed_at: now,
            stake_locked: locked_amount,
        };
        record.highest_bid = Some(bid.clone());
        record.bids.push(bid);

        if let Some(previous) = prev_highest {
            if let (Some(reference), amount) =
                (previous.stake_reference.as_ref(), previous.stake_locked)
            {
                if amount > 0
                    && (Some(reference) != stake_ref_for_bid.as_ref() || previous.bidder != bidder)
                {
                    let mut stake_record = load_stake_or_err(&db, reference)?;
                    stake_record.unlock(amount);
                    record_stake_unlock(amount);
                    persist_stake(&mut db, &stake_record)?;
                }
            }
        }

        let bytes = encode_auction(&record)?;
        db.insert(&key, bytes);

        record
    }; // DNS_DB lock automatically released here

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
            record_auction_cancelled();
            return Err(AuctionError::NoBids);
        }
    };

    let protocol_fee = winning_bid
        .amount
        .saturating_mul(record.protocol_fee_bps as u64)
        / 10_000;

    let royalty_fee = winning_bid.amount.saturating_mul(record.royalty_bps as u64) / 10_000;

    record_treasury_fee(protocol_fee.saturating_add(royalty_fee));

    let ownership_key = ownership_key(domain);
    let ownership = db
        .get(&ownership_key)
        .map(|bytes| decode_ownership(&bytes))
        .transpose()?;

    let mut history = db
        .get(&sale_history_key(domain))
        .map(|bytes| decode_sales(&bytes))
        .transpose()?
        .unwrap_or_default();
    let prior_sale = history.last().cloned();

    let ledger = ledger_handle()?;
    let mut plan: Vec<(LedgerEventKind, LedgerBatchCommand)> = Vec::new();

    if winning_bid.amount > 0 {
        plan.push((
            LedgerEventKind::DebitBidder,
            LedgerBatchCommand::debit(
                &winning_bid.bidder,
                winning_bid.amount,
                &format!("dns_auction_debit:{domain}"),
            ),
        ));
    }

    let seller_payout = winning_bid
        .amount
        .saturating_sub(protocol_fee)
        .saturating_sub(royalty_fee);
    if let Some(stake_ref) = record.seller_stake.as_ref() {
        if let Some(mut stake_record) = load_stake(&db, stake_ref)? {
            let refund_amount = stake_record.unlock(stake_record.locked);
            persist_stake(&mut db, &stake_record)?;
            if refund_amount > 0 {
                record_stake_unlock(refund_amount);
                if let Some(seller) = record.seller_account.as_ref() {
                    plan.push((
                        LedgerEventKind::RefundStake,
                        LedgerBatchCommand::credit(
                            seller,
                            refund_amount,
                            &format!("dns_auction_stake_refund:{domain}"),
                        ),
                    ));
                }
            }
        }
    }

    if let Some(seller) = record.seller_account.as_ref() {
        if seller_payout > 0 {
            plan.push((
                LedgerEventKind::CreditSeller,
                LedgerBatchCommand::credit(
                    seller,
                    seller_payout,
                    &format!("dns_auction_credit:{domain}"),
                ),
            ));
        }
    }

    if royalty_fee > 0 {
        let royalty_recipient = prior_sale
            .as_ref()
            .and_then(|sale| sale.seller_account.clone())
            .or_else(|| ownership.as_ref().map(|owner| owner.owner_account.clone()));
        if let Some(recipient) = royalty_recipient {
            plan.push((
                LedgerEventKind::CreditRoyalty,
                LedgerBatchCommand::credit(
                    &recipient,
                    royalty_fee,
                    &format!("dns_auction_royalty:{domain}"),
                ),
            ));
        } else {
            plan.push((
                LedgerEventKind::CreditRoyalty,
                LedgerBatchCommand::credit_treasury(
                    royalty_fee,
                    &format!("dns_auction_royalty_treasury:{domain}"),
                ),
            ));
        }
    }

    if protocol_fee > 0 {
        plan.push((
            LedgerEventKind::CreditTreasury,
            LedgerBatchCommand::credit_treasury(
                protocol_fee,
                &format!("dns_auction_protocol_fee:{domain}"),
            ),
        ));
    }

    let ledger_events = apply_ledger_plan(&*ledger, plan)?;

    let new_owner = DomainOwnershipRecord {
        domain: domain.to_string(),
        owner_account: winning_bid.bidder.clone(),
        acquired_at: now,
        royalty_bps: ownership
            .as_ref()
            .map(|o| o.royalty_bps)
            .unwrap_or(record.royalty_bps),
        last_sale_price: winning_bid.amount,
        owner_stake: winning_bid.stake_reference.clone(),
    };
    let ownership_bytes = encode_ownership(&new_owner)?;
    db.insert(&ownership_key, ownership_bytes);

    history.push(DomainSaleRecord {
        domain: domain.to_string(),
        sold_at: now,
        seller_account: record.seller_account.clone(),
        buyer_account: winning_bid.bidder.clone(),
        price: winning_bid.amount,
        protocol_fee,
        royalty_fee,
        ledger_events: ledger_events,
    });
    let history_bytes = encode_sales(&history)?;
    db.insert(&sale_history_key(domain), history_bytes);

    record.status = AuctionStatus::Settled;
    record.end_ts = now;
    let bytes = encode_auction(&record)?;
    db.insert(&key, bytes);
    let duration_secs = now.saturating_sub(record.start_ts);
    record_auction_completed(duration_secs, winning_bid.amount);

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        (
            "sale",
            json_map(vec![
                ("domain", Value::String(domain.to_string())),
                ("buyer_account", Value::String(winning_bid.bidder.clone())),
                ("price", Value::Number(Number::from(winning_bid.amount))),
                ("protocol_fee", Value::Number(Number::from(protocol_fee))),
                ("royalty_fee", Value::Number(Number::from(royalty_fee))),
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
    let metrics_window_secs = params
        .get("metrics_window_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);

    // Perform all database operations with lock held, extract data, then release
    let (auctions, ownerships, history, status_counts, next_end_ts, last_end_ts, coverage_demand) = {
        let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
        let mut auctions = Vec::new();
        let mut ownerships = Vec::new();
        let mut history = Vec::new();
        let mut status_counts: HashMap<AuctionStatus, u64> = HashMap::new();
        let mut next_end_ts: Option<u64> = None;
        let mut last_end_ts: Option<u64> = None;
        let mut coverage_demand = 0u64;

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
                let entry = status_counts.entry(record.status).or_insert(0);
                *entry = entry.saturating_add(1);
                if matches!(record.status, AuctionStatus::Active) {
                    next_end_ts = Some(match next_end_ts {
                        Some(current) => current.min(record.end_ts),
                        None => record.end_ts,
                    });
                    last_end_ts = Some(match last_end_ts {
                        Some(current) => current.max(record.end_ts),
                        None => record.end_ts,
                    });
                    let coverage = record
                        .highest_bid
                        .as_ref()
                        .map(|bid| bid.amount)
                        .unwrap_or(record.min_bid);
                    coverage_demand = coverage_demand.saturating_add(coverage);
                }
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

        (
            auctions,
            ownerships,
            history,
            status_counts,
            next_end_ts,
            last_end_ts,
            coverage_demand,
        )
    }; // DNS_DB lock automatically released here

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

    let snapshot = governance_metrics_snapshot(metrics_window_secs);
    let total_locked = total_locked_stake();
    let coverage_ratio_ppm = ppm_ratio(total_locked, coverage_demand);
    let active = *status_counts.get(&AuctionStatus::Active).unwrap_or(&0);
    let settled = *status_counts.get(&AuctionStatus::Settled).unwrap_or(&0);
    let cancelled = *status_counts.get(&AuctionStatus::Cancelled).unwrap_or(&0);
    #[cfg(feature = "telemetry")]
    update_dns_auction_status_metrics(active, settled, cancelled);

    let mut counts_map = Map::new();
    counts_map.insert("active".into(), Value::Number(Number::from(active)));
    counts_map.insert("settled".into(), Value::Number(Number::from(settled)));
    counts_map.insert("cancelled".into(), Value::Number(Number::from(cancelled)));

    let mut timing_map = Map::new();
    timing_map.insert("next_end_ts".into(), value_or_null(next_end_ts));
    timing_map.insert("last_end_ts".into(), value_or_null(last_end_ts));

    let mut stake_map = Map::new();
    stake_map.insert(
        "total_locked".into(),
        Value::Number(Number::from(total_locked)),
    );
    stake_map.insert(
        "coverage_ratio_ppm".into(),
        Value::Number(Number::from(coverage_ratio_ppm)),
    );
    stake_map.insert(
        "coverage_demand".into(),
        Value::Number(Number::from(coverage_demand)),
    );

    let txt_ratio = ppm_ratio(snapshot.txt_successes, snapshot.txt_attempts);
    let completion_ratio = ppm_ratio(
        snapshot.auction_completions,
        snapshot
            .auction_completions
            .saturating_add(snapshot.auction_cancels),
    );
    let metrics_value = json_map(vec![
        (
            "window_secs",
            Value::Number(Number::from(metrics_window_secs)),
        ),
        (
            "txt_attempts",
            Value::Number(Number::from(snapshot.txt_attempts)),
        ),
        (
            "txt_successes",
            Value::Number(Number::from(snapshot.txt_successes)),
        ),
        (
            "txt_success_ratio_ppm",
            Value::Number(Number::from(txt_ratio)),
        ),
        (
            "auction_completions",
            Value::Number(Number::from(snapshot.auction_completions)),
        ),
        (
            "auction_cancels",
            Value::Number(Number::from(snapshot.auction_cancels)),
        ),
        (
            "auction_completion_ratio_ppm",
            Value::Number(Number::from(completion_ratio)),
        ),
        (
            "settlement_stats",
            stats_value(&snapshot.settlement_amounts),
        ),
        (
            "duration_stats",
            stats_value(&snapshot.settle_durations_secs),
        ),
    ]);

    let mut summary_map = Map::new();
    summary_map.insert("auction_counts".into(), Value::Object(counts_map));
    summary_map.insert("timing".into(), Value::Object(timing_map));
    summary_map.insert("stake_snapshot".into(), Value::Object(stake_map));
    summary_map.insert("metrics".into(), metrics_value);

    Ok(json_map(vec![
        ("status", Value::String("ok".to_string())),
        ("auctions", Value::Array(auctions)),
        ("ownership", Value::Array(ownerships)),
        ("history", history_value),
        ("summary", Value::Object(summary_map)),
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
            record_txt_result(*ok);
            return *ok;
        }
    }
    let txts = {
        let resolver = TXT_RESOLVER.lock().unwrap();
        resolver(domain)
    };
    let needle = format!("{}{}", DNS_VERIFICATION_PREFIX, node_id);
    let ok = txts.iter().any(|t| t.contains(&needle));
    VERIFY_CACHE.lock().unwrap().insert(key, (ok, now));
    record_txt_result(ok);
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
    if DISABLE_VERIFY.load(Ordering::Relaxed) {
        return json_map(vec![
            ("record", txt.map(Value::String).unwrap_or(Value::Null)),
            ("verified", Value::Bool(true)),
        ]);
    }
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
    use crate::{Account, Blockchain, TokenBalance};
    use foundation_serialization::json::{Number, Value};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn install_test_chain(accounts: &[(&str, u64)]) -> Arc<Mutex<Blockchain>> {
        let chain = Arc::new(Mutex::new(Blockchain::default()));
        {
            let mut guard = chain.lock().unwrap();
            for (address, balance) in accounts {
                guard.accounts.insert(
                    (*address).to_string(),
                    Account {
                        address: (*address).to_string(),
                        balance: TokenBalance { amount: *balance },
                        nonce: 0,
                        pending_amount: 0,
                        pending_nonce: 0,
                        pending_nonces: HashSet::new(),
                        sessions: Vec::new(),
                    },
                );
            }
            guard
                .accounts
                .entry("treasury".to_string())
                .or_insert(Account {
                    address: "treasury".to_string(),
                    balance: TokenBalance { amount: 0 },
                    nonce: 0,
                    pending_amount: 0,
                    pending_nonce: 0,
                    pending_nonces: HashSet::new(),
                    sessions: Vec::new(),
                });
        }
        install_ledger_context(Arc::new(BlockchainLedger::new(
            Arc::clone(&chain),
            "treasury".to_string(),
        )));
        chain
    }

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

        let _chain = install_test_chain(&[("bidder-main", 5_000)]);
        seed_stake("stake-1", "bidder-main", 2_000);

        let captured = Arc::new(Mutex::new(Vec::new()));
        let hook_capture = Arc::clone(&captured);
        install_treasury_hook(move |amount| {
            hook_capture.lock().unwrap().push(amount);
        });

        let listing = list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(1_000))),
            ("protocol_fee_bps", Value::Number(Number::from(500))),
            ("royalty_bps", Value::Number(Number::from(200))),
        ]))
        .expect("listing ok");
        assert_eq!(listing["status"].as_str(), Some("ok"));

        let low_bid = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-low".to_string())),
            ("bid", Value::Number(Number::from(800))),
        ]));
        assert!(matches!(low_bid, Err(AuctionError::BidTooLow)));

        let winning_bid = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-main".to_string())),
            ("bid", Value::Number(Number::from(1_500))),
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
        assert_eq!(sale["sale"]["price"].as_u64(), Some(1_500));

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
        assert!(history[0]
            .ledger_events
            .iter()
            .any(|event| matches!(event.kind, LedgerEventKind::DebitBidder)));
        assert!(history[0]
            .ledger_events
            .iter()
            .any(|event| matches!(event.kind, LedgerEventKind::CreditTreasury)));
        drop(db);

        {
            let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
            db.remove(&stake_key("stake-1"));
        }
        clear_domain_state(domain);
        clear_ledger_context();
    }

    #[test]
    #[testkit::tb_serial]
    fn resale_respects_royalty_distribution() {
        let domain = "resale-test.block";
        clear_domain_state(domain);

        let _chain = install_test_chain(&[("first-owner", 5_000), ("second-owner", 5_000)]);
        seed_stake("stake-primary", "first-owner", 3_000);
        install_treasury_hook(|_| {});
        // Seed primary sale to establish ownership and royalty rate.
        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(2_000))),
            ("protocol_fee_bps", Value::Number(Number::from(400))),
            ("royalty_bps", Value::Number(Number::from(150))),
        ]))
        .expect("primary listing");
        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("first-owner".to_string())),
            ("bid", Value::Number(Number::from(2_500))),
            (
                "stake_reference",
                Value::String("stake-primary".to_string()),
            ),
        ]))
        .expect("primary winning bid");
        complete_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("force", Value::Bool(true)),
        ]))
        .expect("primary sale");
        clear_treasury_hook();

        seed_stake("stake-second", "second-owner", 4_000);
        let captured = Arc::new(Mutex::new(Vec::new()));
        let hook_capture = Arc::clone(&captured);
        install_treasury_hook(move |amount| {
            hook_capture.lock().unwrap().push(amount);
        });

        let resale_listing = list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(3_000))),
            ("seller_account", Value::String("first-owner".to_string())),
            // Intentionally set royalty to a different value to ensure the stored value persists.
            ("royalty_bps", Value::Number(Number::from(0))),
        ]))
        .expect("resale listing");
        assert_eq!(resale_listing["status"].as_str(), Some("ok"));

        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("second-owner".to_string())),
            ("bid", Value::Number(Number::from(3_600))),
            ("stake_reference", Value::String("stake-second".to_string())),
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
        assert!(history[1]
            .ledger_events
            .iter()
            .any(|event| matches!(event.kind, LedgerEventKind::CreditRoyalty)));
        drop(db);

        {
            let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
            db.remove(&stake_key("stake-primary"));
            db.remove(&stake_key("stake-second"));
        }
        clear_domain_state(domain);
        clear_ledger_context();
    }

    #[testkit::tb_serial]
    fn bid_rejected_after_expiry() {
        let domain = "expiry-test.block";
        clear_domain_state(domain);

        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(500))),
            ("duration_secs", Value::Number(Number::from(0))),
        ]))
        .expect("listing");

        let result = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("late-bid".to_string())),
            ("bid", Value::Number(Number::from(600))),
        ]));
        assert!(matches!(result, Err(AuctionError::AuctionExpired)));

        clear_domain_state(domain);
    }

    #[testkit::tb_serial]
    fn bid_requires_stake_reference() {
        let domain = "stake-required.block";
        clear_domain_state(domain);

        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(750))),
        ]))
        .expect("listing");

        let result = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("no-stake".to_string())),
            ("bid", Value::Number(Number::from(900))),
        ]));
        assert!(matches!(result, Err(AuctionError::BidInsufficientStake)));

        clear_domain_state(domain);
    }

    #[testkit::tb_serial]
    fn bid_rejects_when_stake_insufficient() {
        let domain = "stake-short.block";
        clear_domain_state(domain);

        seed_stake("stake-short", "short-bidder", 500);

        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(1_000))),
        ]))
        .expect("listing");

        let result = place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("short-bidder".to_string())),
            ("bid", Value::Number(Number::from(1_200))),
            ("stake_reference", Value::String("stake-short".to_string())),
        ]));
        assert!(matches!(result, Err(AuctionError::BidInsufficientStake)));

        {
            let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
            db.remove(&stake_key("stake-short"));
        }
        clear_domain_state(domain);
    }

    #[test]
    #[testkit::tb_serial]
    fn outbid_releases_prior_stake() {
        let domain = "stake-release.block";
        clear_domain_state(domain);

        seed_stake("stake-a", "bidder-a", 2_000);
        seed_stake("stake-b", "bidder-b", 2_000);

        list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("min_bid", Value::Number(Number::from(1_000))),
        ]))
        .expect("listing");

        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-a".to_string())),
            ("bid", Value::Number(Number::from(1_200))),
            ("stake_reference", Value::String("stake-a".to_string())),
        ]))
        .expect("initial bid");

        place_bid(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            ("bidder_account", Value::String("bidder-b".to_string())),
            ("bid", Value::Number(Number::from(1_500))),
            ("stake_reference", Value::String("stake-b".to_string())),
        ]))
        .expect("outbid");

        {
            let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
            let record = load_stake(&db, "stake-a")
                .expect("load stake")
                .expect("stake exists");
            assert_eq!(record.locked, 0);
        }

        {
            let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
            db.remove(&stake_key("stake-a"));
            db.remove(&stake_key("stake-b"));
        }
        clear_domain_state(domain);
    }

    #[test]
    fn governance_metrics_snapshot_captures_events() {
        DNS_METRICS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        record_txt_result(true);
        record_txt_result(false);
        record_auction_completed(5, 42);
        record_auction_cancelled();
        record_stake_unlock(10);
        let snap = governance_metrics_snapshot(60);
        assert_eq!(snap.txt_attempts, 2);
        assert_eq!(snap.txt_successes, 1);
        assert_eq!(snap.auction_completions, 1);
        assert_eq!(snap.auction_cancels, 1);
        assert_eq!(snap.stake_unlock_events, 1);
        assert_eq!(snap.settlement_amounts, vec![42]);
    }

    #[test]
    fn dynamic_reserve_pricing_short_domains() {
        // Test that shorter domains get premium pricing
        let base = 1000u64;

        // 3-char domain: 1.0x multiplier
        let price_3char = compute_dynamic_reserve_price("abc", None);
        assert_eq!(
            price_3char, base,
            "3-char domain should get 1.0x base price"
        );

        // 4-char domain: 0.9x multiplier (1.0 - 0.1 * 1)
        let price_4char = compute_dynamic_reserve_price("abcd", None);
        assert_eq!(price_4char, 900, "4-char domain should get 0.9x base price");

        // 5-char domain: 0.8x multiplier
        let price_5char = compute_dynamic_reserve_price("abcde", None);
        assert_eq!(price_5char, 800, "5-char domain should get 0.8x base price");

        // Very long domain: should floor at 0.2x
        let price_long = compute_dynamic_reserve_price("verylongdomainname", None);
        assert_eq!(
            price_long, 200,
            "Long domains should floor at 0.2x base price"
        );
    }

    #[test]
    fn dynamic_reserve_pricing_historical_performance() {
        // Test that domains with successful prior auctions get premium
        let base = 1000u64;

        // Create a prior auction record with high selling price
        let mut prior = DomainAuctionRecord {
            domain: "test.block".to_string(),
            seller_account: Some("seller".to_string()),
            seller_stake: None,
            protocol_fee_bps: 500,
            royalty_bps: 0,
            min_bid: base,
            stake_requirement: base,
            start_ts: 0,
            end_ts: 1000,
            status: AuctionStatus::Settled,
            highest_bid: Some(DomainBidRecord {
                bidder: "winner".to_string(),
                amount: 2000, // 2x base price
                stake_reference: None,
                placed_at: 500,
                stake_locked: 0,
            }),
            bids: vec![],
        };

        // With history_weight = 0.5 and 2x historical price:
        // history_multiplier = 1.0 + 0.5 * (2.0 - 1.0) = 1.5
        // For 4-char domain "test": price = 1000 * 0.9 (length) * 1.5 (history) = 1350
        let price_with_history = compute_dynamic_reserve_price("test", Some(&prior));
        assert_eq!(
            price_with_history, 1350,
            "Should apply historical premium (4-char: 0.9 * 1.5 = 1.35x)"
        );

        // Test with cancelled auction (no history bonus)
        prior.status = AuctionStatus::Cancelled;
        let price_cancelled = compute_dynamic_reserve_price("test", Some(&prior));
        assert_eq!(
            price_cancelled, 900,
            "Cancelled auction should not get history bonus (4-char only)"
        );
    }

    #[test]
    fn dynamic_reserve_pricing_disabled_uses_base() {
        // Test that when disabled, base price is always returned
        {
            let mut config = DYNAMIC_RESERVE_CONFIG
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            config.enabled = false;
            config.base_reserve = 5000;
        }

        let price = compute_dynamic_reserve_price("xyz", None);
        assert_eq!(
            price, 5000,
            "When disabled, should return base price regardless of domain"
        );

        // Re-enable for other tests
        {
            let mut config = DYNAMIC_RESERVE_CONFIG
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            config.enabled = true;
            config.base_reserve = 1000;
        }
    }

    #[testkit::tb_serial]
    fn list_for_sale_uses_dynamic_reserve_when_not_specified() {
        let domain = "xyz.block";
        clear_domain_state(domain);

        let _chain = install_test_chain(&[]);

        // List without specifying min_bid - should use computed reserve
        // "xyz.block" = 9 chars -> length_multiplier = 1.0 - 0.1 * (9-3) = 0.4
        // Expected: 1000 * 0.4 = 400 BLOCK
        let listing = list_for_sale(&json_map(vec![
            ("domain", Value::String(domain.to_string())),
            // min_bid NOT specified - should default to dynamic reserve
        ]))
        .expect("listing ok");

        assert_eq!(listing["status"].as_str(), Some("ok"));

        // Verify the min_bid was set to dynamic reserve price
        let auction = &listing["auction"];
        assert_eq!(
            auction["min_bid"].as_u64(),
            Some(400),
            "Should use dynamic reserve (9-char domain \"xyz.block\": 400 BLOCK)"
        );

        clear_domain_state(domain);
    }
}
