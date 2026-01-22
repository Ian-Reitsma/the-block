#![forbid(unsafe_code)]

use crate::governance::NODE_GOV_STORE;
use crate::simple_db::{names, SimpleDb};
#[cfg(feature = "telemetry")]
use crate::telemetry::energy as energy_metrics;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    ENERGY_ACTIVE_DISPUTES, ENERGY_DISPUTE_OPEN_TOTAL, ENERGY_DISPUTE_RESOLVE_TOTAL,
    ENERGY_METER_READING_TOTAL, ENERGY_PENDING_CREDITS, ENERGY_PROVIDER_REGISTER_TOTAL,
    ENERGY_PROVIDER_TOTAL, ENERGY_SETTLEMENT_TOTAL, ENERGY_TOTAL_RECEIPTS,
    ENERGY_TREASURY_FEE_TOTAL,
};
use concurrency::Lazy;
use crypto_suite::hex;
use diagnostics::tracing::{info, warn};
use energy_market::{
    AccountId, EnergyCredit, EnergyMarket, EnergyMarketConfig, EnergyMarketError, EnergyProvider,
    EnergyReceipt, MeterReading, ProviderId, SettlementMode, SignatureScheme, H256,
};
use foundation_serialization::{binary, Deserialize, Serialize};
use governance_spec::{EnergySettlementMode, EnergySettlementPayload};
use std::io;
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

const KEY_STATE: &str = "state";
const KEY_DISPUTES: &str = "disputes";
const KEY_RECEIPTS: &str = "receipts";
const KEY_SLASHES: &str = "slashes";

#[derive(Clone, Copy, Debug)]
pub struct GovernanceEnergyParams {
    pub min_stake: u64,
    pub oracle_timeout_blocks: u64,
    pub slashing_rate_bps: u16,
    pub settlement: EnergySettlementPayload,
}

impl Default for GovernanceEnergyParams {
    fn default() -> Self {
        Self {
            min_stake: EnergyMarketConfig::default().min_stake,
            oracle_timeout_blocks: EnergyMarketConfig::default().oracle_timeout_blocks,
            slashing_rate_bps: EnergyMarketConfig::default().slashing_rate_bps,
            settlement: EnergySettlementPayload::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", crate = "foundation_serialization::serde")]
pub enum DisputeStatus {
    Open,
    Resolved,
}

impl DisputeStatus {
    fn is_open(self) -> bool {
        matches!(self, DisputeStatus::Open)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergyDispute {
    pub id: u64,
    pub provider_id: String,
    pub meter_hash: H256,
    pub reporter: String,
    pub reason: String,
    pub status: DisputeStatus,
    pub opened_at: u64,
    pub resolved_at: Option<u64>,
    pub resolution_note: Option<String>,
    pub resolver: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct DisputeFilter<'a> {
    pub provider_id: Option<&'a str>,
    pub status: Option<DisputeStatus>,
    pub meter_hash: Option<H256>,
}

impl<'a> Default for DisputeFilter<'a> {
    fn default() -> Self {
        Self {
            provider_id: None,
            status: None,
            meter_hash: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Page<T> {
    pub page: usize,
    pub page_size: usize,
    pub total: usize,
    pub items: Vec<T>,
}

impl<T> Page<T> {
    fn empty(page: usize, page_size: usize) -> Self {
        Self {
            page,
            page_size,
            total: 0,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum DisputeError {
    #[error("meter hash {meter_hash:?} is not tracked by the energy market")]
    UnknownMeterReading { meter_hash: H256 },
    #[error("meter hash {meter_hash:?} already has an open dispute")]
    AlreadyOpen { meter_hash: H256 },
    #[error("dispute {dispute_id} not found")]
    UnknownDispute { dispute_id: u64 },
    #[error("dispute {dispute_id} already resolved")]
    AlreadyResolved { dispute_id: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct DisputeLog {
    next_id: u64,
    entries: Vec<EnergyDispute>,
}

impl Default for DisputeLog {
    fn default() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergySlash {
    pub provider_id: String,
    pub meter_hash: H256,
    pub block_height: u64,
    pub amount: u64,
    pub reason: String,
}

const SLASH_REASON_QUORUM: &str = "quorum";
const SLASH_REASON_EXPIRY: &str = "expiry";
const SLASH_REASON_CONFLICT: &str = "conflict";

struct EnergyMarketStore {
    db: SimpleDb,
    market: EnergyMarket,
    disputes: DisputeLog,
    receipts: Vec<EnergyReceipt>,
    slashes: Vec<EnergySlash>,
}

impl EnergyMarketStore {
    fn open(path: &str) -> Self {
        let db = SimpleDb::open_named(names::ENERGY_MARKET, path);
        let market = db
            .get(KEY_STATE)
            .and_then(|bytes| EnergyMarket::from_bytes(&bytes).ok())
            .unwrap_or_default();
        let disputes = db
            .get(KEY_DISPUTES)
            .and_then(|bytes| binary::decode::<DisputeLog>(&bytes).ok())
            .unwrap_or_default();
        let receipts = db
            .get(KEY_RECEIPTS)
            .and_then(|bytes| binary::decode::<Vec<EnergyReceipt>>(&bytes).ok())
            .unwrap_or_default();
        let slashes = db
            .get(KEY_SLASHES)
            .and_then(|bytes| binary::decode::<Vec<EnergySlash>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            db,
            market,
            disputes,
            receipts,
            slashes,
        }
    }

    fn persist(&mut self) -> io::Result<()> {
        self.persist_market()?;
        self.persist_disputes()?;
        self.persist_receipts()?;
        self.persist_slashes()?;
        Ok(())
    }

    fn persist_market(&mut self) -> io::Result<()> {
        let bytes = self
            .market
            .to_bytes()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.db.insert(KEY_STATE, bytes);
        Ok(())
    }

    fn persist_disputes(&mut self) -> io::Result<()> {
        let bytes = binary::encode(&self.disputes)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.db.insert(KEY_DISPUTES, bytes);
        Ok(())
    }

    fn persist_receipts(&mut self) -> io::Result<()> {
        let bytes = binary::encode(&self.receipts)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.db.insert(KEY_RECEIPTS, bytes);
        Ok(())
    }

    fn persist_slashes(&mut self) -> io::Result<()> {
        let bytes = binary::encode(&self.slashes)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.db.insert(KEY_SLASHES, bytes);
        Ok(())
    }

    fn record_slash(&mut self, slash: EnergySlash) {
        self.slashes.push(slash);
    }

    fn drain_slashes(&mut self) -> Vec<EnergySlash> {
        let drained = self.slashes.clone();
        self.slashes.clear();
        drained
    }

    fn snapshot(&self, governance: GovernanceEnergyParams) -> EnergySnapshot {
        EnergySnapshot {
            providers: self.market.providers().cloned().collect(),
            receipts: self.market.receipts().to_vec(),
            anchored_receipts: self.receipts.clone(),
            credits: self
                .market
                .credits()
                .map(|(_, credit)| credit.clone())
                .collect(),
            disputes: self.disputes.entries.clone(),
            slashes: self.slashes.clone(),
            governance,
        }
    }

    fn next_dispute_id(&mut self) -> u64 {
        let id = self.disputes.next_id;
        self.disputes.next_id = self.disputes.next_id.saturating_add(1);
        id
    }

    fn provider_for_hash(&self, meter_hash: &H256) -> Option<String> {
        if let Some((_, credit)) = self.market.credits().find(|(hash, _)| *hash == meter_hash) {
            return Some(credit.provider.clone());
        }
        if let Some(receipt) = self
            .market
            .receipts()
            .iter()
            .find(|receipt| receipt.meter_reading_hash == *meter_hash)
        {
            return Some(receipt.seller.clone());
        }
        None
    }
}

#[derive(Clone)]
pub struct EnergySnapshot {
    pub providers: Vec<EnergyProvider>,
    pub receipts: Vec<EnergyReceipt>,
    pub anchored_receipts: Vec<EnergyReceipt>,
    pub credits: Vec<EnergyCredit>,
    pub disputes: Vec<EnergyDispute>,
    pub slashes: Vec<EnergySlash>,
    pub governance: GovernanceEnergyParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderKeyConfig {
    pub provider_id: String,
    pub public_key_hex: String,
}

#[derive(Debug, Error)]
pub enum ProviderKeyError {
    #[error("invalid provider key hex for {provider_id}")]
    InvalidHex { provider_id: String },
    #[error("invalid provider key length for {provider_id}: expected 32 bytes, got {len}")]
    InvalidLength { provider_id: String, len: usize },
}

static ENERGY_PARAMS: Lazy<Mutex<GovernanceEnergyParams>> =
    Lazy::new(|| Mutex::new(GovernanceEnergyParams::default()));
static ENERGY_STORE: Lazy<Mutex<EnergyMarketStore>> = Lazy::new(|| {
    let path = std::env::var("TB_ENERGY_MARKET_DIR").unwrap_or_else(|_| "energy_market".into());
    let mut store = EnergyMarketStore::open(&path);
    let params = ENERGY_PARAMS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    apply_params_to_market(&mut store, params);
    #[cfg(feature = "telemetry")]
    record_energy_gauges(&store);
    Mutex::new(store)
});

fn store() -> MutexGuard<'static, EnergyMarketStore> {
    ENERGY_STORE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn persist_or_warn(store: &mut EnergyMarketStore) {
    match store.persist() {
        Ok(()) => {
            #[cfg(feature = "telemetry")]
            record_energy_gauges(store);
        }
        Err(err) => {
            warn!(?err, "failed to persist energy market state");
        }
    }
}

pub fn configure_provider_keys(configs: &[ProviderKeyConfig]) -> Result<(), ProviderKeyError> {
    let mut guard = store();
    guard.market.verifier_registry_mut().clear();
    for cfg in configs {
        let bytes =
            hex::decode(cfg.public_key_hex.trim()).map_err(|_| ProviderKeyError::InvalidHex {
                provider_id: cfg.provider_id.clone(),
            })?;
        if bytes.len() != 32 {
            return Err(ProviderKeyError::InvalidLength {
                provider_id: cfg.provider_id.clone(),
                len: bytes.len(),
            });
        }
        guard.market.register_provider_key(
            cfg.provider_id.clone(),
            bytes,
            SignatureScheme::Ed25519,
        );
    }
    persist_or_warn(&mut guard);
    Ok(())
}

fn apply_params_to_market(store: &mut EnergyMarketStore, params: GovernanceEnergyParams) {
    let mut cfg = store.market.config().clone();
    cfg.min_stake = params.min_stake;
    cfg.oracle_timeout_blocks = if params.settlement.expiry_blocks > 0 {
        params.settlement.expiry_blocks
    } else {
        params.oracle_timeout_blocks
    };
    cfg.slashing_rate_bps = params.slashing_rate_bps;
    cfg.quorum_threshold_ppm = params.settlement.quorum_threshold_ppm;
    cfg.settlement_mode = match params.settlement.mode {
        EnergySettlementMode::Batch => SettlementMode::Batch,
        EnergySettlementMode::RealTime => SettlementMode::RealTime,
    };
    store.market.set_config(cfg);
}

pub fn set_governance_params(params: GovernanceEnergyParams) {
    let mut params = params;
    if let Err(err) = params.settlement.validate() {
        warn!(%err, "invalid energy governance payload; clamping");
        params.settlement.quorum_threshold_ppm =
            params.settlement.quorum_threshold_ppm.min(1_000_000);
    }
    #[cfg(feature = "telemetry")]
    {
        let mode_val = match params.settlement.mode {
            EnergySettlementMode::Batch => 0,
            EnergySettlementMode::RealTime => 1,
        };
        crate::telemetry::ENERGY_SETTLEMENT_MODE.set(mode_val as i64);
    }
    {
        let mut guard = ENERGY_PARAMS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *guard = params;
    }
    let mut guard = store();
    apply_params_to_market(&mut guard, params);
    persist_or_warn(&mut guard);
}

pub fn register_provider(
    owner: AccountId,
    capacity_kwh: u64,
    price_per_kwh: u64,
    meter_address: String,
    jurisdiction: String,
    stake: u64,
) -> Result<EnergyProvider, EnergyMarketError> {
    let mut guard = store();
    let provider_id = guard.market.register_energy_provider(
        owner,
        capacity_kwh,
        price_per_kwh,
        meter_address,
        jurisdiction,
        stake,
    )?;
    let provider = guard
        .market
        .provider(&provider_id)
        .cloned()
        .expect("newly registered provider must exist");
    #[cfg(feature = "telemetry")]
    ENERGY_PROVIDER_REGISTER_TOTAL.inc();
    persist_or_warn(&mut guard);
    Ok(provider)
}

pub fn update_provider(
    provider_id: &str,
    price_per_kwh: Option<u64>,
    capacity_kwh: Option<u64>,
    jurisdiction: Option<String>,
) -> Result<EnergyProvider, EnergyMarketError> {
    let mut guard = store();
    let jurisdiction = jurisdiction.map(|j| j.into());
    let provider = guard.market.update_provider_terms(
        &provider_id.to_string(),
        price_per_kwh,
        capacity_kwh,
        jurisdiction,
    )?;
    persist_or_warn(&mut guard);
    Ok(provider.clone())
}

pub fn submit_meter_reading(
    reading: MeterReading,
    block: u64,
) -> Result<EnergyCredit, EnergyMarketError> {
    // Governance-level expiry: reject meter readings older than the configured window.
    let params = governance_params();
    if params.oracle_timeout_blocks > 0 {
        let age = block.saturating_sub(reading.timestamp);
        if age > params.oracle_timeout_blocks {
            return Err(EnergyMarketError::TimestampSkew {
                provider_id: reading.provider_id.clone(),
                tolerance_secs: params.oracle_timeout_blocks,
                observed_skew: age,
            });
        }
    }
    let mut guard = store();
    let credit = match guard.market.record_meter_reading(reading, block) {
        Ok(credit) => {
            #[cfg(feature = "telemetry")]
            energy_metrics::increment_energy_readings();
            credit
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            {
                let label = match &err {
                    EnergyMarketError::StaleReading { .. } => {
                        energy_metrics::error_reason::STALE_TIMESTAMP
                    }
                    EnergyMarketError::InvalidMeterValue { .. } => {
                        energy_metrics::error_reason::INVALID_READING
                    }
                    EnergyMarketError::SignatureVerificationFailed(_) => {
                        energy_metrics::error_reason::BAD_SIGNATURE
                    }
                    EnergyMarketError::TimestampSkew { .. } => {
                        energy_metrics::error_reason::STALE_TIMESTAMP
                    }
                    EnergyMarketError::NonceReplay { .. } => {
                        energy_metrics::error_reason::INVALID_READING
                    }
                    _ => "other",
                };
                energy_metrics::increment_oracle_submission_error(label);
                energy_metrics::increment_reading_reject(label);
            }
            return Err(err);
        }
    };
    #[cfg(feature = "telemetry")]
    ENERGY_METER_READING_TOTAL
        .with_label_values(&[credit.provider.as_str()])
        .inc();
    persist_or_warn(&mut guard);
    Ok(credit)
}

pub fn settle_energy_delivery(
    buyer: AccountId,
    provider_id: &ProviderId,
    kwh_consumed: u64,
    block: u64,
    meter_hash: H256,
) -> Result<EnergyReceipt, EnergyMarketError> {
    let mut guard = store();
    let receipt = match guard.market.settle_energy_delivery(
        buyer,
        provider_id,
        kwh_consumed,
        block,
        meter_hash,
    ) {
        Ok(receipt) => receipt,
        Err(err) => {
            let mut slash_reason = None;
            let mut slash_hash = meter_hash;
            let mut slash_kwh = kwh_consumed;
            let mut provider_label = Some(provider_id.to_string());
            #[cfg(feature = "telemetry")]
            {
                match &err {
                    EnergyMarketError::SettlementBelowQuorum { .. } => {
                        energy_metrics::increment_quorum_shortfall(provider_id.as_str());
                        energy_metrics::increment_slashing(provider_id.as_str(), "quorum");
                    }
                    EnergyMarketError::CreditExpired(hash) => {
                        let provider = guard
                            .provider_for_hash(hash)
                            .unwrap_or_else(|| provider_id.to_string());
                        energy_metrics::increment_slashing(&provider, "expiry");
                    }
                    EnergyMarketError::UnknownReading(hash) => {
                        let provider = guard
                            .provider_for_hash(hash)
                            .unwrap_or_else(|| provider_id.to_string());
                        energy_metrics::increment_slashing(&provider, "conflict");
                    }
                    _ => {}
                }
            }
            match &err {
                EnergyMarketError::SettlementBelowQuorum { .. } => {
                    slash_reason = Some(SLASH_REASON_QUORUM);
                }
                EnergyMarketError::CreditExpired(hash) => {
                    slash_reason = Some(SLASH_REASON_EXPIRY);
                    slash_hash = *hash;
                    if let Some(provider) = guard.provider_for_hash(hash) {
                        provider_label = Some(provider);
                    }
                }
                EnergyMarketError::UnknownReading(hash) => {
                    slash_reason = Some(SLASH_REASON_CONFLICT);
                    slash_hash = *hash;
                    if let Some(provider) = guard.provider_for_hash(hash) {
                        provider_label = Some(provider);
                    }
                    slash_kwh = 0;
                }
                _ => {}
            }
            if let Some(reason) = slash_reason {
                let provider = provider_label.unwrap_or_else(|| "unknown".into());
                let price_per_kwh = guard
                    .market
                    .provider(&provider)
                    .map(|provider| provider.price_per_kwh)
                    .unwrap_or(0);
                let amount = compute_slash_amount(
                    price_per_kwh,
                    slash_kwh,
                    guard.market.config().slashing_rate_bps,
                );
                record_slash_event(&mut guard, provider, slash_hash, block, amount, reason);
                persist_or_warn(&mut guard);
            }
            return Err(err);
        }
    };
    guard.receipts.push(receipt.clone());
    #[cfg(feature = "telemetry")]
    ENERGY_SETTLEMENT_TOTAL
        .with_label_values(&[provider_id.as_str()])
        .inc();
    persist_or_warn(&mut guard);
    record_treasury_fee(receipt.treasury_fee.saturating_add(receipt.slash_applied));
    Ok(receipt)
}

pub fn drain_energy_receipts() -> Vec<EnergyReceipt> {
    // Drain receipts under lock, then release lock before doing I/O
    let receipts = {
        let mut guard = store();
        guard.market.drain_receipts()
    }; // Lock released here

    // Record telemetry for drain operation
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::receipts::RECEIPT_DRAIN_OPERATIONS_TOTAL.inc();
        if !receipts.is_empty() {
            diagnostics::tracing::debug!(
                receipt_count = receipts.len(),
                market = "energy",
                "Drained energy receipts"
            );
        }
    }

    // Persist market state outside of critical section to avoid blocking
    if !receipts.is_empty() {
        // Re-acquire lock only for persistence
        let mut guard = store();
        if let Err(err) = guard.persist_market() {
            warn!(
                ?err,
                receipt_count = receipts.len(),
                "failed to persist energy market after draining receipts"
            );
        }
    }

    receipts
}

pub fn drain_energy_slash_receipts() -> Vec<EnergySlash> {
    let slashes = {
        let mut guard = store();
        guard.drain_slashes()
    };
    if !slashes.is_empty() {
        let mut guard = store();
        if let Err(err) = guard.persist_slashes() {
            warn!(
                ?err,
                slash_count = slashes.len(),
                "failed to persist energy slashes after draining"
            );
        }
    }
    slashes
}

fn record_treasury_fee(amount: u64) {
    if amount == 0 {
        return;
    }
    #[cfg(feature = "telemetry")]
    ENERGY_TREASURY_FEE_TOTAL.inc_by(amount);
    if let Err(err) = NODE_GOV_STORE.record_treasury_accrual(amount) {
        #[cfg(feature = "telemetry")]
        warn!(amount, ?err, "failed to accrue energy treasury fee");
        #[cfg(not(feature = "telemetry"))]
        let _ = (amount, err);
    }
}

const BASIS_POINTS_DIVISOR: u64 = 10_000;

fn compute_slash_amount(price_per_kwh: u64, kwh: u64, rate_bps: u16) -> u64 {
    if price_per_kwh == 0 || kwh == 0 || rate_bps == 0 {
        return 0;
    }
    let total_cost = (price_per_kwh as u128) * (kwh as u128);
    ((total_cost * rate_bps as u128) / BASIS_POINTS_DIVISOR as u128) as u64
}

fn record_slash_event(
    store: &mut EnergyMarketStore,
    provider_id: String,
    meter_hash: H256,
    block_height: u64,
    amount: u64,
    reason: &'static str,
) {
    let slash = EnergySlash {
        provider_id: provider_id.clone(),
        meter_hash,
        block_height,
        amount,
        reason: reason.into(),
    };
    store.record_slash(slash.clone());
    if amount > 0 {
        record_treasury_fee(amount);
    }
    if let Err(err) = NODE_GOV_STORE.record_energy_slash(
        slash.provider_id.as_str(),
        &slash.meter_hash,
        slash.block_height,
        slash.amount,
        reason,
    ) {
        warn!(?err, "failed to persist energy slash record");
    }
}

pub fn disputes_page<'a>(
    filter: DisputeFilter<'a>,
    page: usize,
    page_size: usize,
) -> Page<EnergyDispute> {
    let guard = store();
    let filtered: Vec<_> = guard
        .disputes
        .entries
        .iter()
        .filter(|entry| matches_dispute(entry, &filter))
        .cloned()
        .collect();
    paginate_from_vec(filtered, page, page_size)
}

pub fn receipts_page(
    provider_id: Option<&str>,
    page: usize,
    page_size: usize,
) -> Page<EnergyReceipt> {
    let guard = store();
    let mut receipts: Vec<_> = guard.market.receipts().to_vec();
    if let Some(provider) = provider_id {
        receipts.retain(|receipt| receipt.seller == provider);
    }
    receipts.sort_by(|a, b| b.block_settled.cmp(&a.block_settled));
    paginate_from_vec(receipts, page, page_size)
}

pub fn slashes_page(provider_id: Option<&str>, page: usize, page_size: usize) -> Page<EnergySlash> {
    let guard = store();
    let mut slashes: Vec<_> = guard
        .slashes
        .iter()
        .filter(|slash| {
            provider_id
                .map(|provider| slash.provider_id == provider)
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    slashes.sort_by(|a, b| b.block_height.cmp(&a.block_height));
    paginate_from_vec(slashes, page, page_size)
}

/// Return anchored receipts (append-only log) for audit/replay.
pub fn anchored_receipts() -> Vec<EnergyReceipt> {
    let guard = store();
    guard.receipts.clone()
}

pub fn credits_page(
    provider_id: Option<&str>,
    page: usize,
    page_size: usize,
) -> Page<EnergyCredit> {
    let guard = store();
    let mut credits: Vec<_> = guard
        .market
        .credits()
        .filter(|(_, credit)| {
            provider_id
                .map(|provider| credit.provider == provider)
                .unwrap_or(true)
        })
        .map(|(_, credit)| credit.clone())
        .collect();
    credits.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    paginate_from_vec(credits, page, page_size)
}

pub fn flag_dispute(
    reporter: String,
    meter_hash: H256,
    reason: String,
    block: u64,
) -> Result<EnergyDispute, DisputeError> {
    let mut guard = store();
    let dispute = flag_dispute_inner(&mut guard, reporter, meter_hash, reason, block)?;
    persist_or_warn(&mut guard);
    Ok(dispute)
}

pub fn resolve_dispute(
    dispute_id: u64,
    resolver: String,
    resolution_note: Option<String>,
    block: u64,
) -> Result<EnergyDispute, DisputeError> {
    let mut guard = store();
    let dispute = resolve_dispute_inner(&mut guard, dispute_id, resolver, resolution_note, block)?;
    persist_or_warn(&mut guard);
    Ok(dispute)
}

pub fn market_snapshot() -> EnergySnapshot {
    let governance = governance_params();
    let guard = store();
    guard.snapshot(governance)
}

pub fn governance_params() -> GovernanceEnergyParams {
    ENERGY_PARAMS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

/// Lightweight health check invoked by telemetry loops to ensure
/// oracle submissions and settlements stay within expected envelopes.
pub fn check_energy_market_health() {
    let guard = store();
    let pending = guard.market.pending_credit_count();
    if pending > 25 {
        warn!(
            pending,
            "energy market pending credits exceed safe threshold; investigate oracle latency"
        );
    }
    let receipts = guard.market.receipt_count();
    if receipts > 0 {
        info!(
            receipts,
            total_kwh = guard.market.total_kwh_settled(),
            "energy market settlements flowing"
        );
    }
}

fn matches_dispute(entry: &EnergyDispute, filter: &DisputeFilter<'_>) -> bool {
    if let Some(status) = filter.status {
        if entry.status != status {
            return false;
        }
    }
    if let Some(provider) = filter.provider_id {
        if entry.provider_id != provider {
            return false;
        }
    }
    if let Some(hash) = filter.meter_hash {
        if entry.meter_hash != hash {
            return false;
        }
    }
    true
}

fn flag_dispute_inner(
    store: &mut EnergyMarketStore,
    reporter: String,
    meter_hash: H256,
    reason: String,
    block: u64,
) -> Result<EnergyDispute, DisputeError> {
    if store
        .disputes
        .entries
        .iter()
        .any(|entry| entry.meter_hash == meter_hash && entry.status.is_open())
    {
        return Err(DisputeError::AlreadyOpen { meter_hash });
    }
    let Some(provider_id) = store.provider_for_hash(&meter_hash) else {
        return Err(DisputeError::UnknownMeterReading { meter_hash });
    };
    let dispute = EnergyDispute {
        id: store.next_dispute_id(),
        provider_id,
        meter_hash,
        reporter,
        reason,
        status: DisputeStatus::Open,
        opened_at: block,
        resolved_at: None,
        resolution_note: None,
        resolver: None,
    };
    store.disputes.entries.push(dispute.clone());
    #[cfg(feature = "telemetry")]
    {
        energy_metrics::increment_dispute_state("open");
        ENERGY_DISPUTE_OPEN_TOTAL.inc();
    }
    Ok(dispute)
}

fn resolve_dispute_inner(
    store: &mut EnergyMarketStore,
    dispute_id: u64,
    resolver: String,
    resolution_note: Option<String>,
    block: u64,
) -> Result<EnergyDispute, DisputeError> {
    let entry = store
        .disputes
        .entries
        .iter_mut()
        .find(|entry| entry.id == dispute_id)
        .ok_or(DisputeError::UnknownDispute { dispute_id })?;
    if !entry.status.is_open() {
        return Err(DisputeError::AlreadyResolved { dispute_id });
    }
    entry.status = DisputeStatus::Resolved;
    entry.resolved_at = Some(block);
    entry.resolution_note = resolution_note;
    entry.resolver = Some(resolver);
    #[cfg(feature = "telemetry")]
    {
        energy_metrics::increment_dispute_state("resolved");
        ENERGY_DISPUTE_RESOLVE_TOTAL.inc();
    }
    Ok(entry.clone())
}

fn clamp_page_size(page_size: usize) -> usize {
    page_size.clamp(1, 250)
}

fn paginate_from_vec<T: Clone>(items: Vec<T>, page: usize, page_size: usize) -> Page<T> {
    if items.is_empty() {
        return Page::empty(page, clamp_page_size(page_size));
    }
    paginate_from_slice(&items, page, page_size)
}

fn paginate_from_slice<T: Clone>(items: &[T], page: usize, page_size: usize) -> Page<T> {
    let page_size = clamp_page_size(page_size);
    let total = items.len();
    let start = page.saturating_mul(page_size).min(total);
    let end = (start + page_size).min(total);
    let slice = items[start..end].to_vec();
    Page {
        page,
        page_size,
        total,
        items: slice,
    }
}

#[cfg(feature = "telemetry")]
fn record_energy_gauges(store: &EnergyMarketStore) {
    ENERGY_PENDING_CREDITS.set(store.market.pending_credit_count() as i64);
    ENERGY_TOTAL_RECEIPTS.set(store.market.receipt_count() as i64);
    let active_disputes = store
        .disputes
        .entries
        .iter()
        .filter(|entry| entry.status.is_open())
        .count() as i64;
    ENERGY_ACTIVE_DISPUTES.set(active_disputes);
    let provider_count = store.market.providers().count() as i64;
    ENERGY_PROVIDER_TOTAL.set(provider_count);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::hashing::blake3::Hasher as Blake3;
    use crypto_suite::signatures::ed25519::SigningKey;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use sys::tempfile::tempdir;

    fn temp_store() -> (sys::tempfile::TempDir, EnergyMarketStore) {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("energy");
        fs::create_dir_all(&path).expect("create dir");
        let path_str = path.to_string_lossy().into_owned();
        let store = EnergyMarketStore::open(&path_str);
        (dir, store)
    }

    fn register_provider(store: &mut EnergyMarketStore) -> (ProviderId, MeterReading) {
        let min_stake = store.market.config().min_stake;
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let verifying = signing.verifying_key();
        let provider_id = store
            .market
            .register_energy_provider(
                "owner-1".into(),
                1_000,
                1,
                "meter-1".into(),
                "US_CA".into(),
                min_stake,
            )
            .expect("register provider");
        store.market.register_provider_key(
            provider_id.clone(),
            verifying.to_bytes().to_vec(),
            SignatureScheme::Ed25519,
        );
        let nonce: u64 = 1;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut hasher = Blake3::new();
        hasher.update(provider_id.as_bytes());
        hasher.update("meter-1".as_bytes());
        hasher.update(&100u64.to_le_bytes());
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(&nonce.to_le_bytes());
        let msg = hasher.finalize();
        let signature = signing.sign(msg.as_bytes()).to_bytes().to_vec();
        let reading = MeterReading {
            provider_id: provider_id.clone(),
            meter_address: "meter-1".into(),
            total_kwh: 100,
            timestamp,
            nonce,
            signature,
        };
        (provider_id, reading)
    }

    #[test]
    fn disputes_can_be_opened_and_resolved() {
        let (_tmp, mut store) = temp_store();
        let (provider_id, reading) = register_provider(&mut store);
        let credit = store
            .market
            .record_meter_reading(reading, 10)
            .expect("credit recorded");
        let dispute = flag_dispute_inner(
            &mut store,
            "reporter-1".into(),
            credit.meter_reading_hash,
            "inaccurate reading".into(),
            12,
        )
        .expect("dispute created");
        assert_eq!(dispute.provider_id, provider_id);
        assert_eq!(dispute.status, DisputeStatus::Open);
        let err = flag_dispute_inner(
            &mut store,
            "reporter-1".into(),
            credit.meter_reading_hash,
            "duplicate".into(),
            13,
        )
        .expect_err("duplicate dispute rejected");
        assert!(matches!(err, DisputeError::AlreadyOpen { .. }));
        let resolved = resolve_dispute_inner(
            &mut store,
            dispute.id,
            "ops".into(),
            Some("refunded buyer".into()),
            20,
        )
        .expect("resolved");
        assert_eq!(resolved.status, DisputeStatus::Resolved);
        assert_eq!(resolved.resolution_note.as_deref(), Some("refunded buyer"));
    }

    #[test]
    fn disputes_require_known_meter_hash() {
        let (_tmp, mut store) = temp_store();
        let meter_hash = [0u8; 32];
        let err = flag_dispute_inner(
            &mut store,
            "reporter-1".into(),
            meter_hash,
            "invalid".into(),
            5,
        )
        .expect_err("unknown meter hash rejected");
        assert!(matches!(err, DisputeError::UnknownMeterReading { .. }));
    }

    #[test]
    fn provider_updates_apply() {
        let (_dir, mut store) = temp_store();
        let provider = store
            .market
            .register_energy_provider(
                "owner-2".into(),
                1_000,
                2,
                "meter-2".into(),
                "US_CA".into(),
                store.market.config().min_stake,
            )
            .expect("register provider");
        store.persist().expect("persist initial");
        let mut guard = ENERGY_STORE.lock().unwrap();
        let previous = std::mem::replace(&mut *guard, store);
        drop(guard);
        let updated = update_provider(&provider, Some(5), Some(2_000), Some("US_WA".into()))
            .expect("update provider");
        assert_eq!(updated.price_per_kwh, 5);
        assert_eq!(updated.capacity_kwh, 2_000);
        assert_eq!(updated.location, "US_WA");
        let mut guard = ENERGY_STORE.lock().unwrap();
        *guard = previous;
    }
}
