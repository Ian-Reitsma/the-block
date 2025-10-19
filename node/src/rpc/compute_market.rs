use crate::compute_market::{
    matcher, price_board,
    receipt::Receipt,
    scheduler,
    settlement::{BalanceSnapshot, Settlement, SettlementEngineInfo},
    total_units_processed,
};
use crate::transaction::FeeLane;
use foundation_serialization::json::{self, Map, Value};
use foundation_serialization::Serialize;
use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeMarketStatsResponse {
    pub industrial_backlog: u64,
    pub industrial_utilization: u64,
    pub industrial_units_total: u64,
    pub industrial_price_per_unit: u64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub industrial_price_weighted: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub industrial_price_base: Option<u64>,
    pub pending: Vec<scheduler::PendingJob>,
    pub lanes: Vec<ComputeLaneStatus>,
    pub lane_starvation: Vec<ComputeLaneWarning>,
    pub recent_matches: BTreeMap<String, Vec<ComputeRecentMatch>>,
    pub settlement_engine: SettlementEngineInfo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeLaneStatus {
    pub lane: String,
    pub bids: usize,
    pub asks: usize,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub oldest_bid_wait_ms: Option<u128>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub oldest_ask_wait_ms: Option<u128>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeLaneWarning {
    pub lane: String,
    pub job_id: String,
    pub waited_for_secs: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeRecentMatch {
    pub job_id: String,
    pub provider: String,
    pub buyer: String,
    pub price: u64,
    pub issued_at: u64,
    pub lane: String,
}

impl From<matcher::LaneStatus> for ComputeLaneStatus {
    fn from(value: matcher::LaneStatus) -> Self {
        Self {
            lane: value.lane.as_str().to_string(),
            bids: value.bids,
            asks: value.asks,
            oldest_bid_wait_ms: value.oldest_bid_wait.map(|d| d.as_millis()),
            oldest_ask_wait_ms: value.oldest_ask_wait.map(|d| d.as_millis()),
        }
    }
}

impl From<matcher::LaneWarning> for ComputeLaneWarning {
    fn from(value: matcher::LaneWarning) -> Self {
        let updated_at = value
            .updated_at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        Self {
            lane: value.lane.as_str().to_string(),
            job_id: value.oldest_job,
            waited_for_secs: value.waited_for.as_secs(),
            updated_at,
        }
    }
}

impl From<Receipt> for ComputeRecentMatch {
    fn from(value: Receipt) -> Self {
        Self {
            job_id: value.job_id,
            provider: value.provider,
            buyer: value.buyer,
            price: value.quote_price,
            issued_at: value.issued_at,
            lane: value.lane.as_str().to_string(),
        }
    }
}

/// Return compute market backlog and utilisation metrics.
pub fn stats(_accel: Option<crate::compute_market::Accelerator>) -> ComputeMarketStatsResponse {
    let (backlog, util) = price_board::backlog_utilization();
    let weighted = price_board::bands(FeeLane::Industrial).map(|(_, m, _)| m);
    let raw = price_board::raw_bands(FeeLane::Industrial).map(|(_, m, _)| m);
    let spot = price_board::spot_price_per_unit(FeeLane::Industrial)
        .or(weighted)
        .or(raw)
        .unwrap_or_default();
    let sched = scheduler::stats();
    let lane_status = matcher::lane_statuses();
    let lane_warnings = matcher::starvation_warnings();
    let mut recent: BTreeMap<String, Vec<ComputeRecentMatch>> = BTreeMap::new();
    for status in &lane_status {
        let receipts = matcher::recent_matches(status.lane, 5);
        let entries: Vec<ComputeRecentMatch> =
            receipts.into_iter().map(ComputeRecentMatch::from).collect();
        recent.insert(status.lane.as_str().to_string(), entries);
    }
    let lanes = lane_status
        .into_iter()
        .map(ComputeLaneStatus::from)
        .collect();
    let warnings = lane_warnings
        .into_iter()
        .map(ComputeLaneWarning::from)
        .collect();
    let settlement_engine = Settlement::engine_info();
    ComputeMarketStatsResponse {
        industrial_backlog: backlog,
        industrial_utilization: util,
        industrial_units_total: total_units_processed(),
        industrial_price_per_unit: spot,
        industrial_price_weighted: weighted,
        industrial_price_base: raw,
        pending: sched.pending,
        lanes,
        lane_starvation: warnings,
        recent_matches: recent,
        settlement_engine,
    }
}

/// Return scheduler reputation and capability utilisation metrics.
pub fn scheduler_metrics() -> Value {
    scheduler::metrics()
}

/// Return aggregated scheduler statistics over recent matches.
pub fn scheduler_stats() -> Value {
    json::to_value(scheduler::stats()).unwrap()
}

/// Return current reputation score for a provider.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReputationResponse<'a> {
    pub provider: &'a str,
    pub score: i64,
}

pub fn reputation_get(provider: &str) -> ReputationResponse<'_> {
    ReputationResponse {
        provider,
        score: scheduler::reputation_get(provider),
    }
}

/// Return capability requirements for an active job.
pub fn job_requirements(job_id: &str) -> Value {
    if let Some(cap) = scheduler::job_requirements(job_id) {
        json::to_value(cap).unwrap()
    } else {
        Value::Object(Map::new())
    }
}

/// Cancel an active job and release resources.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct StatusResponse {
    pub status: &'static str,
}

pub fn job_cancel(job_id: &str) -> StatusResponse {
    if let Some(provider) = scheduler::active_provider(job_id) {
        scheduler::cancel_job(job_id, &provider, scheduler::CancelReason::Client);
        StatusResponse { status: "ok" }
    } else {
        StatusResponse { status: "unknown" }
    }
}

/// Return advertised hardware capability for a provider.
pub fn provider_hardware(provider: &str) -> Value {
    if let Some(cap) = scheduler::provider_capability(provider) {
        json::to_value(cap).unwrap()
    } else {
        Value::Object(Map::new())
    }
}

/// Return the recent settlement audit log.
pub fn settlement_audit() -> Value {
    json::to_value(Settlement::audit()).unwrap_or_else(|_| Value::Array(Vec::new()))
}

/// Return split token balances for providers.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderBalancesResponse {
    pub providers: Vec<BalanceSnapshot>,
}

pub fn provider_balances() -> ProviderBalancesResponse {
    ProviderBalancesResponse {
        providers: Settlement::balances(),
    }
}

/// Return recent settlement merkle roots encoded as hex strings.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct RecentRootsResponse {
    pub roots: Vec<String>,
}

pub fn recent_roots(limit: usize) -> RecentRootsResponse {
    let roots: Vec<String> = Settlement::recent_roots(limit)
        .into_iter()
        .map(|r| crypto_suite::hex::encode(r))
        .collect();
    RecentRootsResponse { roots }
}
