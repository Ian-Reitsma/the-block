use super::json_map;
use crate::compute_market::{
    matcher, price_board,
    receipt::Receipt,
    scheduler,
    settlement::{AuditRecord, BalanceSnapshot, Settlement, SettlementEngineInfo, SlaResolution},
    snark::{ProofBundle, SnarkBackend},
    total_units_processed,
};
use crate::transaction::FeeLane;
use crypto_suite::hex;
use foundation_serialization::json::{Map, Number, Value};
use foundation_serialization::Serialize;
use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchStats {
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub kernel_digest: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub benchmark_commit: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub tensor_profile_epoch: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub proof_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub aggregator_trace: Option<String>,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocktorch: Option<BlockTorchStats>,
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

fn optional_u64_value(value: Option<u64>) -> Value {
    value
        .map(|v| Value::Number(Number::from(v)))
        .unwrap_or(Value::Null)
}

fn optional_string_value(value: Option<&String>) -> Value {
    value
        .map(|v| Value::String(v.clone()))
        .unwrap_or(Value::Null)
}

fn accelerator_value(accelerator: Option<crate::compute_market::Accelerator>) -> Value {
    accelerator
        .map(|acc| {
            Value::String(
                match acc {
                    crate::compute_market::Accelerator::Fpga => "FPGA",
                    crate::compute_market::Accelerator::Tpu => "TPU",
                }
                .to_string(),
            )
        })
        .unwrap_or(Value::Null)
}

fn frameworks_value(frameworks: &[String]) -> Value {
    Value::Array(frameworks.iter().cloned().map(Value::String).collect())
}

fn capability_to_value(capability: &scheduler::Capability) -> Value {
    json_map(vec![
        (
            "cpu_cores",
            Value::Number(Number::from(capability.cpu_cores)),
        ),
        ("gpu", optional_string_value(capability.gpu.as_ref())),
        (
            "gpu_memory_mb",
            Value::Number(Number::from(capability.gpu_memory_mb)),
        ),
        (
            "accelerator",
            accelerator_value(capability.accelerator.clone()),
        ),
        (
            "accelerator_memory_mb",
            Value::Number(Number::from(capability.accelerator_memory_mb)),
        ),
        ("frameworks", frameworks_value(&capability.frameworks)),
    ])
}

fn pending_job_to_value(job: &scheduler::PendingJob) -> Value {
    let priority = match job.priority {
        scheduler::Priority::Low => "Low",
        scheduler::Priority::Normal => "Normal",
        scheduler::Priority::High => "High",
    };
    let effective = Number::from_f64(job.effective_priority)
        .expect("scheduler effective priority must be finite");

    json_map(vec![
        ("job_id", Value::String(job.job_id.clone())),
        ("priority", Value::String(priority.to_string())),
        ("effective_priority", Value::Number(effective)),
    ])
}

fn proof_backend_label(backend: SnarkBackend) -> &'static str {
    match backend {
        SnarkBackend::Cpu => "CPU",
        SnarkBackend::Gpu => "GPU",
    }
}

fn proof_bundle_to_value(bundle: &ProofBundle) -> Value {
    let mut map = Map::new();
    map.insert(
        "backend".to_string(),
        Value::String(proof_backend_label(bundle.backend).to_string()),
    );
    map.insert(
        "fingerprint".to_string(),
        Value::String(hex::encode(bundle.fingerprint())),
    );
    map.insert(
        "latency_ms".to_string(),
        Value::Number(Number::from(bundle.latency_ms)),
    );
    map.insert(
        "circuit_hash".to_string(),
        Value::String(hex::encode(bundle.circuit_hash)),
    );
    map.insert(
        "program_commitment".to_string(),
        Value::String(hex::encode(bundle.program_commitment)),
    );
    map.insert(
        "output_commitment".to_string(),
        Value::String(hex::encode(bundle.output_commitment)),
    );
    map.insert(
        "witness_commitment".to_string(),
        Value::String(hex::encode(bundle.witness_commitment)),
    );
    map.insert(
        "artifact".to_string(),
        json_map(vec![
            (
                "circuit_hash",
                Value::String(hex::encode(bundle.artifact.circuit_hash)),
            ),
            (
                "wasm_hash",
                Value::String(hex::encode(bundle.artifact.wasm_hash)),
            ),
            (
                "generated_at",
                Value::Number(Number::from(bundle.artifact.generated_at)),
            ),
        ]),
    );
    map.insert("verified".to_string(), Value::Bool(bundle.self_check()));
    map.insert(
        "proof".to_string(),
        Value::String(hex::encode(&bundle.encoded)),
    );
    Value::Object(map)
}

fn sla_resolution_to_value(resolution: &SlaResolution) -> Value {
    let (status, reason) = match &resolution.outcome {
        crate::compute_market::settlement::SlaResolutionKind::Completed => ("completed", None),
        crate::compute_market::settlement::SlaResolutionKind::Cancelled { reason } => {
            ("cancelled", Some(reason))
        }
        crate::compute_market::settlement::SlaResolutionKind::Violated { reason } => {
            ("violated", Some(reason))
        }
    };
    let proofs = resolution
        .proofs
        .iter()
        .map(proof_bundle_to_value)
        .collect();
    let mut map = Map::new();
    map.insert(
        "job_id".to_string(),
        Value::String(resolution.job_id.clone()),
    );
    map.insert(
        "provider".to_string(),
        Value::String(resolution.provider.clone()),
    );
    map.insert("buyer".to_string(), Value::String(resolution.buyer.clone()));
    map.insert("outcome".to_string(), Value::String(status.to_string()));
    if let Some(reason) = reason {
        map.insert("outcome_reason".to_string(), Value::String(reason.clone()));
    }
    map.insert(
        "burned".to_string(),
        Value::Number(Number::from(resolution.burned)),
    );
    map.insert(
        "refunded".to_string(),
        Value::Number(Number::from(resolution.refunded)),
    );
    map.insert(
        "deadline".to_string(),
        Value::Number(Number::from(resolution.deadline)),
    );
    map.insert(
        "resolved_at".to_string(),
        Value::Number(Number::from(resolution.resolved_at)),
    );
    map.insert("proofs".to_string(), Value::Array(proofs));
    Value::Object(map)
}

fn utilization_to_value(utilization: std::collections::HashMap<String, u64>) -> Value {
    let mut entries: Vec<_> = utilization.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut map = Map::new();
    for (lane, value) in entries {
        map.insert(lane, Value::Number(Number::from(value)));
    }
    Value::Object(map)
}

fn scheduler_stats_to_value(stats: scheduler::SchedulerStats) -> Value {
    let pending = stats.pending.iter().map(pending_job_to_value).collect();
    json_map(vec![
        ("success", Value::Number(Number::from(stats.success))),
        (
            "capability_mismatch",
            Value::Number(Number::from(stats.capability_mismatch)),
        ),
        (
            "reputation_failure",
            Value::Number(Number::from(stats.reputation_failure)),
        ),
        (
            "preemptions",
            Value::Number(Number::from(stats.preemptions)),
        ),
        (
            "active_jobs",
            Value::Number(Number::from(stats.active_jobs)),
        ),
        ("utilization", utilization_to_value(stats.utilization)),
        ("effective_price", optional_u64_value(stats.effective_price)),
        (
            "queued_high",
            Value::Number(Number::from(stats.queued_high)),
        ),
        (
            "queued_normal",
            Value::Number(Number::from(stats.queued_normal)),
        ),
        ("queued_low", Value::Number(Number::from(stats.queued_low))),
        (
            "priority_miss",
            Value::Number(Number::from(stats.priority_miss)),
        ),
        ("pending", Value::Array(pending)),
    ])
}

fn audit_record_to_value(record: &AuditRecord) -> Value {
    let mut map = Map::new();
    map.insert(
        "sequence".to_string(),
        Value::Number(Number::from(record.sequence)),
    );
    map.insert(
        "timestamp".to_string(),
        Value::Number(Number::from(record.timestamp)),
    );
    map.insert("entity".to_string(), Value::String(record.entity.clone()));
    map.insert("memo".to_string(), Value::String(record.memo.clone()));
    map.insert(
        "delta".to_string(),
        Value::Number(Number::from(record.delta)),
    );
    map.insert(
        "balance".to_string(),
        Value::Number(Number::from(record.balance)),
    );
    if let Some(anchor) = &record.anchor {
        map.insert("anchor".to_string(), Value::String(anchor.clone()));
    }
    Value::Object(map)
}

#[cfg(feature = "telemetry")]
fn blocktorch_stats() -> Option<BlockTorchStats> {
    let meta = crate::telemetry::blocktorch_metadata_snapshot();
    if meta.is_empty() {
        None
    } else {
        Some(BlockTorchStats {
            kernel_digest: meta.kernel_digest,
            benchmark_commit: meta.benchmark_commit,
            tensor_profile_epoch: meta.tensor_profile_epoch,
            proof_latency_ms: meta.proof_latency_ms,
            aggregator_trace: meta.aggregator_trace,
        })
    }
}

#[cfg(not(feature = "telemetry"))]
fn blocktorch_stats() -> Option<BlockTorchStats> {
    None
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
        blocktorch: blocktorch_stats(),
    }
}

/// Return scheduler reputation and capability utilisation metrics.
pub fn scheduler_metrics() -> Value {
    scheduler::metrics()
}

/// Return aggregated scheduler statistics over recent matches.
pub fn scheduler_stats() -> Value {
    scheduler_stats_to_value(scheduler::stats())
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
        capability_to_value(&cap)
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
        capability_to_value(&cap)
    } else {
        Value::Object(Map::new())
    }
}

/// Return the recent settlement audit log.
pub fn settlement_audit() -> Value {
    let records = Settlement::audit();
    let values = records.iter().map(audit_record_to_value).collect();
    Value::Array(values)
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

/// Return recent SLA resolutions along with recorded SNARK proofs.
pub fn sla_history(limit: usize) -> Value {
    let entries = Settlement::sla_history(limit);
    Value::Array(entries.iter().map(sla_resolution_to_value).collect())
}
