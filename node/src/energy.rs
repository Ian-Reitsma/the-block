#![forbid(unsafe_code)]

use crate::governance::NODE_GOV_STORE;
use crate::simple_db::{names, SimpleDb};
use concurrency::Lazy;
use diagnostics::tracing::{info, warn};
use energy_market::{
    AccountId, EnergyCredit, EnergyMarket, EnergyMarketConfig, EnergyMarketError, EnergyProvider,
    EnergyReceipt, MeterReading, ProviderId, H256,
};
use std::io;
use std::sync::{Mutex, MutexGuard};

const KEY_STATE: &str = "state";

#[derive(Clone, Copy, Debug)]
pub struct GovernanceEnergyParams {
    pub min_stake: u64,
    pub oracle_timeout_blocks: u64,
    pub slashing_rate_bps: u16,
}

impl Default for GovernanceEnergyParams {
    fn default() -> Self {
        Self {
            min_stake: EnergyMarketConfig::default().min_stake,
            oracle_timeout_blocks: EnergyMarketConfig::default().oracle_timeout_blocks,
            slashing_rate_bps: EnergyMarketConfig::default().slashing_rate_bps,
        }
    }
}

struct EnergyMarketStore {
    db: SimpleDb,
    market: EnergyMarket,
}

impl EnergyMarketStore {
    fn open(path: &str) -> Self {
        let db = SimpleDb::open_named(names::ENERGY_MARKET, path);
        let market = db
            .get(KEY_STATE)
            .and_then(|bytes| EnergyMarket::from_bytes(&bytes).ok())
            .unwrap_or_default();
        Self { db, market }
    }

    fn persist(&mut self) -> io::Result<()> {
        let bytes = self
            .market
            .to_bytes()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.db.insert(KEY_STATE, bytes);
        Ok(())
    }

    fn snapshot(&self) -> EnergySnapshot {
        EnergySnapshot {
            providers: self.market.providers().cloned().collect(),
            receipts: self.market.receipts().to_vec(),
            credits: self
                .market
                .credits()
                .map(|(_, credit)| credit.clone())
                .collect(),
        }
    }
}

#[derive(Clone)]
pub struct EnergySnapshot {
    pub providers: Vec<EnergyProvider>,
    pub receipts: Vec<EnergyReceipt>,
    pub credits: Vec<EnergyCredit>,
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
    Mutex::new(store)
});

fn store() -> MutexGuard<'static, EnergyMarketStore> {
    ENERGY_STORE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn persist_or_warn(store: &mut EnergyMarketStore) {
    if let Err(err) = store.persist() {
        warn!(?err, "failed to persist energy market state");
    }
}

fn apply_params_to_market(store: &mut EnergyMarketStore, params: GovernanceEnergyParams) {
    let mut cfg = store.market.config().clone();
    cfg.min_stake = params.min_stake;
    cfg.oracle_timeout_blocks = params.oracle_timeout_blocks;
    cfg.slashing_rate_bps = params.slashing_rate_bps;
    store.market.set_config(cfg);
}

pub fn set_governance_params(params: GovernanceEnergyParams) {
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
    persist_or_warn(&mut guard);
    Ok(provider)
}

pub fn submit_meter_reading(
    reading: MeterReading,
    block: u64,
) -> Result<EnergyCredit, EnergyMarketError> {
    let mut guard = store();
    let credit = guard.market.record_meter_reading(reading, block)?;
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
    let receipt =
        guard
            .market
            .settle_energy_delivery(buyer, provider_id, kwh_consumed, block, meter_hash)?;
    persist_or_warn(&mut guard);
    record_treasury_fee(receipt.treasury_fee.saturating_add(receipt.slash_applied));
    Ok(receipt)
}

fn record_treasury_fee(amount_ct: u64) {
    if amount_ct == 0 {
        return;
    }
    if let Err(err) = NODE_GOV_STORE.record_treasury_accrual(amount_ct, 0) {
        #[cfg(feature = "telemetry")]
        warn!(amount_ct, ?err, "failed to accrue energy treasury fee");
        #[cfg(not(feature = "telemetry"))]
        let _ = (amount_ct, err);
    }
}

pub fn market_snapshot() -> EnergySnapshot {
    let guard = store();
    guard.snapshot()
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
