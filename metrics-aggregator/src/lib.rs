use concurrency::Lazy;
use diagnostics::{
    internal::{install_tls_env_warning_subscriber, SubscriberGuard as LoggingSubscriberGuard},
    tracing::{info, warn},
};
use foundation_metrics::{gauge, increment_counter, Recorder, RecorderInstallError};
use governance::{
    codec::{balance_history_from_json, disbursements_from_json_array},
    DisbursementStatus, GovStore, TreasuryBalanceEventKind, TreasuryBalanceSnapshot,
    TreasuryDisbursement,
};
use http_env::{http_client as env_http_client, register_tls_warning_sink, TlsEnvWarningSinkGuard};
use httpd::metrics as http_metrics;
use httpd::uri::form_urlencoded;
use httpd::{HttpClient, HttpError, Method, Request, Response, Router, StatusCode};
use runtime::telemetry::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntGaugeVec,
    Opts, Registry,
};
use runtime::{spawn, spawn_blocking};
use std::convert::TryFrom;
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;
use sys::archive::zip::ZipBuilder;

use crypto_suite::encryption::{
    envelope::{self, EnvelopeError, PASSWORD_CONTENT_TYPE, RECIPIENT_CONTENT_TYPE},
    x25519,
};

#[cfg(feature = "s3")]
mod object_store;

mod leader;

pub use leader::LeaderElectionConfig;

#[cfg(feature = "s3")]
fn upload_sync(bucket: &str, data: Vec<u8>) {
    if let Err(err) = object_store::upload_metrics_snapshot(bucket, data) {
        warn!(
            target: "aggregator",
            error = %err,
            "failed to upload metrics snapshot"
        );
    }
}

use foundation_serialization::json;
use foundation_serialization::json::{Map, Number, Value};
use foundation_telemetry::{
    MemorySnapshotEntry, TelemetrySummary, ValidationError, WrapperMetricEntry, WrapperSummaryEntry,
};
use std::collections::{btree_map::Entry, BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use storage_engine::{inhouse_engine::InhouseEngine, KeyValue, KeyValueIterator};
use tls_warning::{
    detail_fingerprint as tls_detail_fingerprint, fingerprint_label,
    variables_fingerprint as tls_variables_fingerprint, WarningOrigin,
};

fn http_client() -> HttpClient {
    env_http_client(&["TB_AGGREGATOR_TLS", "TB_HTTP_TLS"], "metrics-aggregator")
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct BridgeHttpOverrideResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub trait BridgeHttpClientOverride: Send + Sync {
    fn send(&self, url: &str, payload: &Value) -> Result<BridgeHttpOverrideResponse, String>;
}

type BridgeHttpOverrideHandle = Arc<dyn BridgeHttpClientOverride>;

static BRIDGE_HTTP_CLIENT_OVERRIDE: Lazy<Mutex<Option<BridgeHttpOverrideHandle>>> =
    Lazy::new(|| Mutex::new(None));

fn bridge_http_client_override() -> Option<BridgeHttpOverrideHandle> {
    BRIDGE_HTTP_CLIENT_OVERRIDE
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Arc::clone))
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct BridgeHttpClientOverrideGuard {
    previous: Option<BridgeHttpOverrideHandle>,
}

impl Drop for BridgeHttpClientOverrideGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = BRIDGE_HTTP_CLIENT_OVERRIDE.lock() {
            *guard = self.previous.take();
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn install_bridge_http_client_override(
    client: BridgeHttpOverrideHandle,
) -> BridgeHttpClientOverrideGuard {
    let mut guard = BRIDGE_HTTP_CLIENT_OVERRIDE
        .lock()
        .expect("bridge http override lock");
    let previous = guard.replace(client);
    BridgeHttpClientOverrideGuard { previous }
}

fn archive_metrics(blob: &str) {
    if let Ok(path) = std::env::var("TB_METRICS_ARCHIVE") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{}", blob);
        }
    }
}

const MAX_CORRELATIONS_PER_METRIC: usize = 64;
const TELEMETRY_WINDOW: usize = 120;
const METRICS_CF: &str = "peer_metrics";
const BRIDGE_ANOMALY_CF: &str = "bridge_anomaly_state";
const BRIDGE_ANOMALY_STATE_KEY: &[u8] = b"bridge_anomaly_snapshot";
const BRIDGE_REMEDIATION_CF: &str = "bridge_remediation_state";
const BRIDGE_REMEDIATION_STATE_KEY: &[u8] = b"bridge_remediation_snapshot";
const COUNTER_EPSILON: f64 = 1e-6;
const TLS_WARNING_SNAPSHOT_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;
static TLS_WARNING_RETENTION_SECS: AtomicU64 = AtomicU64::new(TLS_WARNING_SNAPSHOT_RETENTION_SECS);
static BRIDGE_REMEDIATION_DISPATCH_SEQ: AtomicU64 = AtomicU64::new(1);
const BRIDGE_REMEDIATION_MAX_DISPATCH_LOG: usize = 256;
const BRIDGE_REMEDIATION_RUNBOOK_PATH: &str =
    "docs/operators/incident_playbook.md#bridge-liquidity-remediation";
const BRIDGE_REMEDIATION_DISPATCH_ENDPOINT: &str = "/remediation/bridge/dispatches";
const BRIDGE_REMEDIATION_ACK_PANEL: &str = "bridge_remediation_dispatch_ack_total (5m delta)";
const BRIDGE_REMEDIATION_ACK_LATENCY_PANEL: &str =
    "bridge_remediation_ack_latency_seconds (p50/p95)";
const BRIDGE_REMEDIATION_BASE_PANELS: &[&str] = &[
    "bridge_remediation_action_total (5m delta)",
    "bridge_remediation_dispatch_total (5m delta)",
    BRIDGE_REMEDIATION_ACK_PANEL,
    BRIDGE_REMEDIATION_ACK_LATENCY_PANEL,
];
const BRIDGE_LIQUIDITY_PANELS: &[&str] = &[
    "bridge_liquidity_locked_total (5m delta)",
    "bridge_liquidity_unlocked_total (5m delta)",
    "bridge_liquidity_minted_total (5m delta)",
    "bridge_liquidity_burned_total (5m delta)",
];
const BRIDGE_PANEL_REWARD_CLAIMS: &str = "bridge_reward_claims_total (5m delta)";
const BRIDGE_PANEL_REWARD_APPROVALS: &str = "bridge_reward_approvals_consumed_total (5m delta)";
const BRIDGE_PANEL_SETTLEMENT_RESULTS: &str = "bridge_settlement_results_total (5m delta)";
const BRIDGE_PANEL_DISPUTE_OUTCOMES: &str = "bridge_dispute_outcomes_total (5m delta)";

const METRIC_AGGREGATOR_INGEST_TOTAL: &str = "aggregator_ingest_total";
const METRIC_BULK_EXPORT_TOTAL: &str = "bulk_export_total";
const METRIC_CLUSTER_PEER_ACTIVE_TOTAL: &str = "cluster_peer_active_total";
const METRIC_AGGREGATOR_REPLICATION_LAG: &str = "aggregator_replication_lag_seconds";
const METRIC_AGGREGATOR_RETENTION_PRUNED_TOTAL: &str = "aggregator_retention_pruned_total";
const METRIC_TELEMETRY_INGEST_TOTAL: &str = "aggregator_telemetry_ingest_total";
const METRIC_TELEMETRY_SCHEMA_ERROR_TOTAL: &str = "aggregator_telemetry_schema_error_total";
const METRIC_TLS_ENV_WARNING_TOTAL: &str = "tls_env_warning_total";
const METRIC_TLS_ENV_WARNING_EVENTS_TOTAL: &str = "tls_env_warning_events_total";
const METRIC_TLS_ENV_WARNING_LAST_SEEN: &str = "tls_env_warning_last_seen_seconds";
const METRIC_TLS_ENV_WARNING_RETENTION_SECONDS: &str = "tls_env_warning_retention_seconds";
const METRIC_TLS_ENV_WARNING_ACTIVE_SNAPSHOTS: &str = "tls_env_warning_active_snapshots";
const METRIC_TLS_ENV_WARNING_STALE_SNAPSHOTS: &str = "tls_env_warning_stale_snapshots";
const METRIC_TLS_ENV_WARNING_MOST_RECENT_LAST_SEEN: &str =
    "tls_env_warning_most_recent_last_seen_seconds";
const METRIC_TLS_ENV_WARNING_LEAST_RECENT_LAST_SEEN: &str =
    "tls_env_warning_least_recent_last_seen_seconds";
const METRIC_TLS_ENV_WARNING_DETAIL_FINGERPRINT: &str = "tls_env_warning_detail_fingerprint";
const METRIC_TLS_ENV_WARNING_VARIABLES_FINGERPRINT: &str = "tls_env_warning_variables_fingerprint";
const METRIC_TLS_ENV_WARNING_DETAIL_FINGERPRINT_TOTAL: &str =
    "tls_env_warning_detail_fingerprint_total";
const METRIC_TLS_ENV_WARNING_VARIABLES_FINGERPRINT_TOTAL: &str =
    "tls_env_warning_variables_fingerprint_total";
const METRIC_TLS_ENV_WARNING_DETAIL_UNIQUE_FINGERPRINTS: &str =
    "tls_env_warning_detail_unique_fingerprints";
const METRIC_TLS_ENV_WARNING_VARIABLES_UNIQUE_FINGERPRINTS: &str =
    "tls_env_warning_variables_unique_fingerprints";
const METRIC_BRIDGE_ANOMALY_TOTAL: &str = "bridge_anomaly_total";
const METRIC_BRIDGE_COUNTER_DELTA: &str = "bridge_metric_delta";
const METRIC_BRIDGE_COUNTER_RATE: &str = "bridge_metric_rate_per_second";
const METRIC_BRIDGE_REMEDIATION_ACTION_TOTAL: &str = "bridge_remediation_action_total";
const METRIC_BRIDGE_REMEDIATION_DISPATCH_TOTAL: &str = "bridge_remediation_dispatch_total";
const METRIC_BRIDGE_REMEDIATION_DISPATCH_ACK_TOTAL: &str = "bridge_remediation_dispatch_ack_total";
const METRIC_BRIDGE_REMEDIATION_ACK_LATENCY_SECONDS: &str =
    "bridge_remediation_ack_latency_seconds";
const METRIC_BRIDGE_REMEDIATION_ACK_TARGET_SECONDS: &str = "bridge_remediation_ack_target_seconds";
const METRIC_RUNTIME_SPAWN_LATENCY: &str = "runtime_spawn_latency_seconds";
const METRIC_RUNTIME_PENDING_TASKS: &str = "runtime_pending_tasks";
const METRIC_TREASURY_COUNT: &str = "treasury_disbursement_count";
const METRIC_TREASURY_AMOUNT_CT: &str = "treasury_disbursement_amount_ct";
const METRIC_TREASURY_SNAPSHOT_AGE: &str = "treasury_disbursement_snapshot_age_seconds";
const METRIC_TREASURY_SCHEDULED_OLDEST_AGE: &str =
    "treasury_disbursement_scheduled_oldest_age_seconds";
const METRIC_TREASURY_NEXT_EPOCH: &str = "treasury_disbursement_next_epoch";
const METRIC_TREASURY_BALANCE_CURRENT: &str = "treasury_balance_current_ct";
const METRIC_TREASURY_BALANCE_LAST_DELTA: &str = "treasury_balance_last_delta_ct";
const METRIC_TREASURY_BALANCE_SNAPSHOT_COUNT: &str = "treasury_balance_snapshot_count";
const METRIC_TREASURY_BALANCE_EVENT_AGE: &str = "treasury_balance_last_event_age_seconds";
const TREASURY_STATUS_LABELS: [&str; 3] = ["scheduled", "executed", "cancelled"];

const LABEL_PREFIX_CODE: [&str; 2] = ["prefix", "code"];
const LABEL_PREFIX_CODE_ORIGIN: [&str; 3] = ["prefix", "code", "origin"];
const LABEL_PREFIX_CODE_FINGERPRINT: [&str; 3] = ["prefix", "code", "fingerprint"];
const LABEL_BRIDGE_COUNTER: [&str; 3] = ["metric", "peer", "labels"];
const LABEL_REMEDIATION_ACTION: [&str; 2] = ["action", "playbook"];
const LABEL_REMEDIATION_DISPATCH: [&str; 4] = ["action", "playbook", "target", "status"];
const LABEL_REMEDIATION_ACK: [&str; 4] = ["action", "playbook", "target", "state"];
const LABEL_REMEDIATION_ACK_TARGET: [&str; 2] = ["playbook", "phase"];

const BRIDGE_MONITORED_COUNTERS: [&str; 8] = [
    "bridge_reward_claims_total",
    "bridge_reward_approvals_consumed_total",
    "bridge_settlement_results_total",
    "bridge_dispute_outcomes_total",
    "bridge_liquidity_locked_total",
    "bridge_liquidity_unlocked_total",
    "bridge_liquidity_minted_total",
    "bridge_liquidity_burned_total",
];
const BRIDGE_ANOMALY_WINDOW: usize = 24;
const BRIDGE_ANOMALY_BASELINE_MIN: usize = 6;
const BRIDGE_ANOMALY_STD_MULTIPLIER: f64 = 4.0;
const BRIDGE_ANOMALY_MIN_STDDEV: f64 = 1.0;
const BRIDGE_ANOMALY_MIN_DELTA: f64 = 5.0;
const BRIDGE_ANOMALY_COOLDOWN_SECS: u64 = 15 * 60;
const BRIDGE_ANOMALY_MAX_EVENTS: usize = 200;
const BRIDGE_REMEDIATION_WINDOW_SECS: u64 = 30 * 60;
const BRIDGE_REMEDIATION_PAGE_COOLDOWN_SECS: u64 = 15 * 60;
const BRIDGE_REMEDIATION_MAX_ACTIONS: usize = 200;
const BRIDGE_REMEDIATION_PAGE_DELTA: f64 = 5.0;
const BRIDGE_REMEDIATION_PAGE_RATIO: f64 = 1.0;
const BRIDGE_REMEDIATION_THROTTLE_DELTA: f64 = 15.0;
const BRIDGE_REMEDIATION_THROTTLE_RATIO: f64 = 1.5;
const BRIDGE_REMEDIATION_THROTTLE_COUNT: usize = 2;
const BRIDGE_REMEDIATION_QUARANTINE_DELTA: f64 = 25.0;
const BRIDGE_REMEDIATION_QUARANTINE_RATIO: f64 = 2.0;
const BRIDGE_REMEDIATION_QUARANTINE_COUNT: usize = 3;
const BRIDGE_REMEDIATION_ESCALATE_DELTA: f64 = 80.0;
const BRIDGE_REMEDIATION_ESCALATE_RATIO: f64 = 4.0;
const BRIDGE_REMEDIATION_ESCALATE_COUNT: usize = 5;
const BRIDGE_REMEDIATION_ACK_RETRY_SECS: u64 = 5 * 60;
const BRIDGE_REMEDIATION_ACK_ESCALATE_SECS: u64 = 15 * 60;
const BRIDGE_REMEDIATION_ACK_MAX_RETRIES: u32 = 3;
const ENV_REMEDIATION_ACK_RETRY_SECS: &str = "TB_REMEDIATION_ACK_RETRY_SECS";
const ENV_REMEDIATION_ACK_ESCALATE_SECS: &str = "TB_REMEDIATION_ACK_ESCALATE_SECS";
const ENV_REMEDIATION_ACK_MAX_RETRIES: &str = "TB_REMEDIATION_ACK_MAX_RETRIES";
const ENV_AGGREGATOR_CLEANUP_INTERVAL_SECS: &str = "AGGREGATOR_CLEANUP_INTERVAL_SECS";

const ENV_REMEDIATION_PAGE_URLS: &str = "TB_REMEDIATION_PAGE_URLS";
const ENV_REMEDIATION_PAGE_DIRS: &str = "TB_REMEDIATION_PAGE_DIRS";
const ENV_REMEDIATION_THROTTLE_URLS: &str = "TB_REMEDIATION_THROTTLE_URLS";
const ENV_REMEDIATION_THROTTLE_DIRS: &str = "TB_REMEDIATION_THROTTLE_DIRS";
const ENV_REMEDIATION_QUARANTINE_URLS: &str = "TB_REMEDIATION_QUARANTINE_URLS";
const ENV_REMEDIATION_QUARANTINE_DIRS: &str = "TB_REMEDIATION_QUARANTINE_DIRS";
const ENV_REMEDIATION_ESCALATE_URLS: &str = "TB_REMEDIATION_ESCALATE_URLS";
const ENV_REMEDIATION_ESCALATE_DIRS: &str = "TB_REMEDIATION_ESCALATE_DIRS";

#[derive(Clone)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: Value,
}

impl PeerStat {
    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "peer stat entry must be an object".to_string())?;
        let peer_id = object
            .get("peer_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "peer stat entry missing peer_id".to_string())?;
        let metrics = object
            .get("metrics")
            .cloned()
            .ok_or_else(|| "peer stat entry missing metrics".to_string())?;
        Ok(Self {
            peer_id: peer_id.to_string(),
            metrics,
        })
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("peer_id".to_string(), Value::String(self.peer_id.clone()));
        map.insert("metrics".to_string(), self.metrics.clone());
        Value::Object(map)
    }
}

fn parse_peer_stats(bytes: &[u8]) -> Result<Vec<PeerStat>, HttpError> {
    let value = json::value_from_slice(bytes).map_err(HttpError::from)?;
    let array = value
        .as_array()
        .ok_or_else(|| HttpError::Handler("ingest payload must be an array".to_string()))?;
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        let stat = PeerStat::from_value(entry).map_err(HttpError::Handler)?;
        out.push(stat);
    }
    Ok(out)
}

fn peer_stats_to_value(stats: &[PeerStat]) -> Value {
    let entries = stats.iter().map(PeerStat::to_value).collect();
    Value::Array(entries)
}

fn json_response(status: StatusCode, value: Value) -> Result<Response, HttpError> {
    let body = json::to_vec_value(&value);
    Ok(Response::new(status)
        .with_header("content-type", "application/json")
        .with_body(body))
}

fn json_ok(value: Value) -> Result<Response, HttpError> {
    json_response(StatusCode::OK, value)
}

struct TelemetryErrorResponse {
    error: String,
    path: String,
}

impl TelemetryErrorResponse {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("error".to_string(), Value::String(self.error.clone()));
        map.insert("path".to_string(), Value::String(self.path.clone()));
        Value::Object(map)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorrelationRecord {
    pub metric: String,
    pub correlation_id: String,
    pub peer_id: String,
    pub value: Option<f64>,
    pub timestamp: u64,
}

impl CorrelationRecord {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("metric".to_string(), Value::String(self.metric.clone()));
        map.insert(
            "correlation_id".to_string(),
            Value::String(self.correlation_id.clone()),
        );
        map.insert("peer_id".to_string(), Value::String(self.peer_id.clone()));
        match self.value {
            Some(v) => map.insert("value".to_string(), Value::from(v)),
            None => map.insert("value".to_string(), Value::Null),
        };
        map.insert("timestamp".to_string(), Value::from(self.timestamp));
        Value::Object(map)
    }
}

#[derive(Clone, Debug)]
struct RawCorrelation {
    metric: String,
    correlation_id: String,
    value: Option<f64>,
}

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<Mutex<HashMap<String, VecDeque<(u64, Value)>>>>,
    pub token: Arc<RwLock<String>>,
    token_path: Option<PathBuf>,
    store: Arc<InhouseEngine>,
    db_path: Arc<PathBuf>,
    retention_secs: u64,
    max_export_peers: usize,
    wal: Option<Arc<Wal>>,
    correlations: Arc<Mutex<HashMap<String, VecDeque<CorrelationRecord>>>>,
    last_metric_values: Arc<Mutex<HashMap<(String, String), f64>>>,
    telemetry: Arc<Mutex<HashMap<String, VecDeque<TelemetrySummary>>>>,
    tls_warning_counters: Arc<Mutex<HashMap<(String, String, String), f64>>>,
    bridge_anomalies: Arc<Mutex<BridgeAnomalyDetector>>,
    bridge_remediation: Arc<Mutex<BridgeRemediationEngine>>,
    bridge_hooks: BridgeRemediationHooks,
    leader_flag: Arc<AtomicBool>,
    leader_id: Arc<RwLock<Option<String>>>,
    leader_fencing: Arc<AtomicU64>,
    treasury_source: Option<TreasurySource>,
}

#[derive(Clone)]
enum TreasurySource {
    Json(PathBuf),
    Store(GovStore),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderSnapshot {
    pub is_leader: bool,
    pub leader_id: Option<String>,
    pub fencing_token: u64,
}

impl AppState {
    pub fn new(token: String, path: impl AsRef<Path>, retention_secs: u64) -> Self {
        Self::new_with_opts(token, None, path, retention_secs, None, None, None)
    }

    pub fn new_with_opts(
        token: String,
        token_path: Option<PathBuf>,
        path: impl AsRef<Path>,
        retention_secs: u64,
        wal: Option<PathBuf>,
        tls_warning_retention_secs: Option<u64>,
        treasury_path: Option<PathBuf>,
    ) -> Self {
        ensure_foundation_metrics_recorder();
        let db_path = path.as_ref().to_path_buf();
        let store = Arc::new(
            InhouseEngine::open(&db_path.to_string_lossy()).expect("open inhouse metrics store"),
        );
        store.ensure_cf(METRICS_CF).expect("ensure cf");
        store
            .ensure_cf(BRIDGE_ANOMALY_CF)
            .expect("ensure bridge anomaly cf");
        store
            .ensure_cf(BRIDGE_REMEDIATION_CF)
            .expect("ensure bridge remediation cf");
        let mut data = HashMap::new();
        let mut iter = store
            .prefix_iterator(METRICS_CF, &[])
            .expect("scan metrics store");
        while let Some((k, v)) = iter.next().expect("iterate metrics store") {
            if let Ok(key) = String::from_utf8(k) {
                if let Ok(deque) = json::from_slice(&v) {
                    data.insert(key, deque);
                }
            }
        }
        let wal = wal.and_then(|p| Wal::open(p).ok()).map(Arc::new);
        let retention = tls_warning_retention_secs.unwrap_or(TLS_WARNING_SNAPSHOT_RETENTION_SECS);
        TLS_WARNING_RETENTION_SECS.store(retention, Ordering::Relaxed);
        gauge!(METRIC_TLS_ENV_WARNING_RETENTION_SECONDS, retention as f64);
        let treasury_source = match env::var("AGGREGATOR_TREASURY_DB") {
            Ok(path) if !path.is_empty() => Some(TreasurySource::Store(GovStore::open(path))),
            _ => treasury_path.clone().map(TreasurySource::Json),
        };
        let state = Self {
            data: Arc::new(Mutex::new(data)),
            token: Arc::new(RwLock::new(token)),
            token_path,
            store,
            db_path: Arc::new(db_path),
            retention_secs,
            max_export_peers: 1000,
            wal,
            correlations: Arc::new(Mutex::new(HashMap::new())),
            last_metric_values: Arc::new(Mutex::new(HashMap::new())),
            telemetry: Arc::new(Mutex::new(HashMap::new())),
            tls_warning_counters: Arc::new(Mutex::new(HashMap::new())),
            bridge_anomalies: Arc::new(Mutex::new(BridgeAnomalyDetector::default())),
            bridge_remediation: Arc::new(Mutex::new(BridgeRemediationEngine::default())),
            bridge_hooks: BridgeRemediationHooks::from_env(),
            leader_flag: Arc::new(AtomicBool::new(false)),
            leader_id: Arc::new(RwLock::new(None)),
            leader_fencing: Arc::new(AtomicU64::new(0)),
            treasury_source,
        };
        state.load_bridge_anomaly_state();
        state.load_bridge_remediation_state();
        state.prune();
        state.refresh_treasury_metrics();
        state
    }

    fn persist(&self) {
        let _ = self.store.flush();
    }

    fn prune(&self) -> u64 {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(self.retention_secs);
        let mut removed = 0u64;
        if let Ok(mut map) = self.data.lock() {
            map.retain(|peer, deque| {
                let before = deque.len();
                deque.retain(|(ts, _)| *ts >= cutoff);
                let after = deque.len();
                removed += (before - after) as u64;
                if after == 0 {
                    let _ = self.store.delete(METRICS_CF, peer.as_bytes());
                    false
                } else {
                    let value = json::to_vec(deque).unwrap();
                    let _ = self.store.put_bytes(METRICS_CF, peer.as_bytes(), &value);
                    true
                }
            });
        }
        if removed > 0 {
            increment_counter!(METRIC_AGGREGATOR_RETENTION_PRUNED_TOTAL, removed);
            let _ = self.store.flush();
        }
        removed
    }

    fn refresh_treasury_metrics(&self) {
        let metrics = aggregator_metrics();
        let Some(source) = &self.treasury_source else {
            Self::reset_treasury_metrics(metrics);
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        match source {
            TreasurySource::Json(path) => match load_treasury_records(path) {
                Ok(records) => {
                    let summary = TreasurySummary::from_records(&records);
                    Self::apply_disbursement_metrics(metrics, &summary, now);
                    match load_treasury_balance_history(path) {
                        Ok(history) => {
                            if history.is_empty() && !records.is_empty() {
                                warn!(
                                    target: "aggregator",
                                    path = %balance_history_path(path).display(),
                                    "treasury disbursements present but no balance snapshots found"
                                );
                            }
                            Self::apply_balance_metrics(metrics, &history, None, now);
                        }
                        Err(err) => {
                            warn!(
                                target: "aggregator",
                                error = %err,
                                path = %balance_history_path(path).display(),
                                "failed to refresh treasury balance history"
                            );
                            Self::zero_balance_metrics(metrics);
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        target: "aggregator",
                        error = %err,
                        path = %path.display(),
                        "failed to refresh treasury metrics"
                    );
                    Self::reset_treasury_metrics(metrics);
                }
            },
            TreasurySource::Store(store) => {
                match (
                    store.disbursements(),
                    store.treasury_balance_history(),
                    store.treasury_balance(),
                ) {
                    (Ok(records), Ok(history), Ok(current_balance)) => {
                        let summary = TreasurySummary::from_records(&records);
                        Self::apply_disbursement_metrics(metrics, &summary, now);
                        if history.is_empty() && !records.is_empty() {
                            warn!(
                                target: "aggregator",
                                "treasury store reported disbursements without balance history"
                            );
                        }
                        Self::apply_balance_metrics(metrics, &history, Some(current_balance), now);
                    }
                    (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
                        warn!(
                            target: "aggregator",
                            error = %err,
                            "failed to refresh treasury store metrics"
                        );
                        Self::reset_treasury_metrics(metrics);
                    }
                }
            }
        }
    }

    fn apply_disbursement_metrics(
        metrics: &AggregatorMetrics,
        summary: &TreasurySummary,
        now: u64,
    ) {
        for status in TREASURY_STATUS_LABELS {
            let (count, amount) = summary.metrics_for_status(status);
            metrics
                .treasury_disbursement_count
                .with_label_values(&[status])
                .set(count as f64);
            metrics
                .treasury_disbursement_amount
                .with_label_values(&[status])
                .set(amount as f64);
        }
        metrics
            .treasury_disbursement_snapshot_age
            .set(summary.snapshot_age(now) as f64);
        metrics
            .treasury_disbursement_scheduled_oldest_age
            .set(summary.scheduled_oldest_age(now) as f64);
        metrics
            .treasury_disbursement_next_epoch
            .set(summary.next_epoch_value() as f64);
    }

    fn apply_balance_metrics(
        metrics: &AggregatorMetrics,
        history: &[TreasuryBalanceSnapshot],
        balance_override: Option<u64>,
        now: u64,
    ) {
        let current_balance = balance_override
            .or_else(|| history.last().map(|snap| snap.balance_ct))
            .unwrap_or(0);
        metrics.treasury_balance_current.set(current_balance as f64);
        let last_delta = history
            .last()
            .map(|snap| snap.delta_ct as f64)
            .unwrap_or(0.0);
        metrics.treasury_balance_last_delta.set(last_delta);
        metrics
            .treasury_balance_snapshot_count
            .set(history.len() as f64);
        let age = history
            .last()
            .map(|snap| now.saturating_sub(snap.recorded_at))
            .unwrap_or(0);
        metrics.treasury_balance_last_event_age.set(age as f64);
    }

    fn zero_disbursement_metrics(metrics: &AggregatorMetrics) {
        for status in TREASURY_STATUS_LABELS {
            metrics
                .treasury_disbursement_count
                .with_label_values(&[status])
                .set(0.0);
            metrics
                .treasury_disbursement_amount
                .with_label_values(&[status])
                .set(0.0);
        }
        metrics.treasury_disbursement_snapshot_age.set(0.0);
        metrics.treasury_disbursement_scheduled_oldest_age.set(0.0);
        metrics.treasury_disbursement_next_epoch.set(0.0);
    }

    fn zero_balance_metrics(metrics: &AggregatorMetrics) {
        metrics.treasury_balance_current.set(0.0);
        metrics.treasury_balance_last_delta.set(0.0);
        metrics.treasury_balance_snapshot_count.set(0.0);
        metrics.treasury_balance_last_event_age.set(0.0);
    }

    fn reset_treasury_metrics(metrics: &AggregatorMetrics) {
        Self::zero_disbursement_metrics(metrics);
        Self::zero_balance_metrics(metrics);
    }

    fn current_token(&self) -> String {
        if let Some(path) = &self.token_path {
            if let Ok(t) = std::fs::read_to_string(path) {
                let mut guard = self.token.write().unwrap();
                let t = t.trim().to_string();
                if *guard != t {
                    *guard = t.clone();
                }
            }
        }
        self.token.read().unwrap().clone()
    }

    pub fn spawn_cleanup(&self) {
        let state = self.clone();
        spawn(async move {
            state.refresh_treasury_metrics();
            state.poll_bridge_followups();
            let interval_secs = env::var(ENV_AGGREGATOR_CLEANUP_INTERVAL_SECS)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(60);
            let mut ticker = runtime::interval(Duration::from_secs(interval_secs));
            loop {
                ticker.tick().await;
                state.prune();
                state.refresh_treasury_metrics();
                state.poll_bridge_followups();
            }
        });
    }

    pub fn weekly_report(&self) -> String {
        if let Ok(map) = self.data.lock() {
            format!("active_peers:{}", map.len())
        } else {
            "active_peers:0".into()
        }
    }

    fn load_bridge_anomaly_state(&self) {
        let snapshot = match self.store.get(BRIDGE_ANOMALY_CF, BRIDGE_ANOMALY_STATE_KEY) {
            Ok(Some(bytes)) => match json::from_slice(&bytes) {
                Ok(value) => Some(value),
                Err(err) => {
                    warn!(
                        target: "aggregator",
                        error = %err,
                        "failed to decode bridge anomaly snapshot",
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(err) => {
                warn!(
                    target: "aggregator",
                    ?err,
                    "failed to load bridge anomaly snapshot",
                );
                None
            }
        };
        if let Some(value) = snapshot {
            match self.bridge_anomalies.lock() {
                Ok(mut detector) => detector.restore(&value),
                Err(_) => warn!(
                    target: "aggregator",
                    "bridge anomaly detector poisoned during snapshot load"
                ),
            }
        }
    }

    fn load_bridge_remediation_state(&self) {
        let snapshot = match self
            .store
            .get(BRIDGE_REMEDIATION_CF, BRIDGE_REMEDIATION_STATE_KEY)
        {
            Ok(Some(bytes)) => match json::from_slice(&bytes) {
                Ok(value) => Some(value),
                Err(err) => {
                    warn!(
                        target: "aggregator",
                        error = %err,
                        "failed to decode bridge remediation snapshot",
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(err) => {
                warn!(
                    target: "aggregator",
                    ?err,
                    "failed to load bridge remediation snapshot",
                );
                None
            }
        };
        if let Some(value) = snapshot {
            let observations = match self.bridge_remediation.lock() {
                Ok(mut engine) => {
                    engine.restore(&value);
                    engine.ack_latency_observations()
                }
                Err(_) => {
                    warn!(
                        target: "aggregator",
                        "bridge remediation engine poisoned during snapshot load"
                    );
                    Vec::new()
                }
            };
            if !observations.is_empty() {
                let metrics = aggregator_metrics();
                for sample in observations {
                    let handle = metrics
                        .bridge_remediation_ack_latency_seconds
                        .with_label_values(&[sample.playbook.as_str(), sample.state.as_str()]);
                    for _ in 0..sample.count {
                        handle.observe(sample.latency as f64);
                    }
                }
            }
        }
    }

    fn persist_bridge_anomaly_snapshot(&self, snapshot: &Value) {
        match json::to_vec(snapshot) {
            Ok(bytes) => {
                if let Err(err) =
                    self.store
                        .put_bytes(BRIDGE_ANOMALY_CF, BRIDGE_ANOMALY_STATE_KEY, &bytes)
                {
                    warn!(
                        target: "aggregator",
                        ?err,
                        "failed to persist bridge anomaly snapshot",
                    );
                }
            }
            Err(err) => warn!(
                target: "aggregator",
                error = %err,
                "failed to encode bridge anomaly snapshot",
            ),
        }
    }

    fn persist_bridge_remediation_snapshot(&self, snapshot: &Value) {
        match json::to_vec(snapshot) {
            Ok(bytes) => {
                if let Err(err) = self.store.put_bytes(
                    BRIDGE_REMEDIATION_CF,
                    BRIDGE_REMEDIATION_STATE_KEY,
                    &bytes,
                ) {
                    warn!(
                        target: "aggregator",
                        ?err,
                        "failed to persist bridge remediation snapshot",
                    );
                }
            }
            Err(err) => warn!(
                target: "aggregator",
                error = %err,
                "failed to encode bridge remediation snapshot",
            ),
        }
    }

    fn record_bridge_anomalies(&self, peer_id: &str, metrics: &Value, timestamp: u64) {
        let (result, snapshot) = match self.bridge_anomalies.lock() {
            Ok(mut detector) => {
                let result = detector.ingest(peer_id, metrics, timestamp);
                let snapshot = detector.snapshot();
                (result, Some(snapshot))
            }
            Err(_) => (BridgeIngestResult::default(), None),
        };
        if let Some(snapshot) = snapshot {
            self.persist_bridge_anomaly_snapshot(&snapshot);
        }
        for observation in &result.observations {
            let labels = if observation.labels.is_empty() {
                String::new()
            } else {
                observation
                    .labels
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            gauge!(
                METRIC_BRIDGE_COUNTER_DELTA,
                observation.delta,
                "metric" => observation.metric.clone(),
                "peer" => observation.peer.clone(),
                "labels" => labels.clone(),
            );
            gauge!(
                METRIC_BRIDGE_COUNTER_RATE,
                observation.rate_per_sec,
                "metric" => observation.metric.clone(),
                "peer" => observation.peer.clone(),
                "labels" => labels,
            );
        }
        if result.events.is_empty() {
            return;
        }
        for event in &result.events {
            increment_counter!(METRIC_BRIDGE_ANOMALY_TOTAL);
            let labels = event
                .labels
                .iter()
                .map(|label| format!("{}={}", label.key, label.value))
                .collect::<Vec<_>>()
                .join(",");
            warn!(
                target: "aggregator",
                metric = %event.metric,
                peer = %event.peer_id,
                delta = event.delta,
                mean = event.mean,
                stddev = event.stddev,
                threshold = event.threshold,
                labels = %labels,
                "bridge anomaly detected"
            );
            self.record_bridge_remediation(event);
        }
    }

    fn bridge_anomaly_events(&self) -> Vec<BridgeAnomalyEvent> {
        self.bridge_anomalies
            .lock()
            .map(|detector| detector.events())
            .unwrap_or_default()
    }

    fn record_bridge_remediation(&self, event: &BridgeAnomalyEvent) {
        let (action, snapshot) = match self.bridge_remediation.lock() {
            Ok(mut engine) => {
                let action = engine.ingest(event);
                let snapshot = engine.snapshot();
                (action, Some(snapshot))
            }
            Err(_) => (None, None),
        };
        if let Some(snapshot) = snapshot {
            self.persist_bridge_remediation_snapshot(&snapshot);
        }
        if let Some(action) = action {
            self.dispatch_bridge_action(&action, BridgeRemediationDispatchOrigin::Anomaly);
        }
    }

    fn dispatch_bridge_action(
        &self,
        action: &BridgeRemediationAction,
        origin: BridgeRemediationDispatchOrigin,
    ) {
        let metrics = aggregator_metrics();
        if matches!(
            origin,
            BridgeRemediationDispatchOrigin::Anomaly
                | BridgeRemediationDispatchOrigin::AutoEscalation
        ) {
            metrics
                .bridge_remediation_action_total
                .with_label_values(&[action.action.as_str(), action.playbook.as_str()])
                .inc();
        }
        let labels = action
            .labels
            .iter()
            .map(|label| format!("{}={}", label.key, label.value))
            .collect::<Vec<_>>()
            .join(",");
        match origin {
            BridgeRemediationDispatchOrigin::Anomaly => {
                warn!(
                    target: "aggregator",
                    peer = %action.peer_id,
                    metric = %action.metric,
                    action = action.action.as_str(),
                    playbook = action.playbook.as_str(),
                    occurrences = action.occurrences,
                    delta = action.delta,
                    threshold = action.threshold,
                    ratio = action.ratio,
                    labels = %labels,
                    "bridge remediation action emitted",
                );
            }
            BridgeRemediationDispatchOrigin::AutoRetry => {
                let pending_since = action
                    .pending_since
                    .or(action.first_dispatch_at)
                    .unwrap_or(action.timestamp);
                warn!(
                    target: "aggregator",
                    peer = %action.peer_id,
                    metric = %action.metric,
                    action = action.action.as_str(),
                    playbook = action.playbook.as_str(),
                    attempts = action.dispatch_attempts,
                    retry_count = action.auto_retry_count,
                    pending_since = pending_since,
                    follow_up = action.follow_up_notes.as_deref().unwrap_or(""),
                    labels = %labels,
                    "bridge remediation acknowledgement pending – retrying dispatch",
                );
            }
            BridgeRemediationDispatchOrigin::AutoEscalation => {
                warn!(
                    target: "aggregator",
                    peer = %action.peer_id,
                    metric = %action.metric,
                    action = action.action.as_str(),
                    playbook = action.playbook.as_str(),
                    follow_up = action.follow_up_notes.as_deref().unwrap_or(""),
                    labels = %labels,
                    "bridge remediation acknowledgement escalation emitted",
                );
            }
        }
        self.bridge_hooks.dispatch(self.clone(), action);
    }

    fn poll_bridge_followups(&self) {
        let now = unix_timestamp_secs();
        let (followups, snapshot) = match self.bridge_remediation.lock() {
            Ok(mut engine) => {
                let followups = engine.pending_followups(now);
                let snapshot = if followups.is_empty() {
                    None
                } else {
                    Some(engine.snapshot())
                };
                (followups, snapshot)
            }
            Err(_) => {
                warn!(
                    target: "aggregator",
                    "bridge remediation engine poisoned while evaluating follow-ups",
                );
                return;
            }
        };
        if let Some(snapshot) = snapshot {
            self.persist_bridge_remediation_snapshot(&snapshot);
        }
        for followup in followups {
            match followup {
                BridgeRemediationFollowUp::Retry { action } => {
                    self.dispatch_bridge_action(
                        &action,
                        BridgeRemediationDispatchOrigin::AutoRetry,
                    );
                }
                BridgeRemediationFollowUp::Escalate { escalation } => {
                    self.dispatch_bridge_action(
                        &escalation,
                        BridgeRemediationDispatchOrigin::AutoEscalation,
                    );
                }
            }
        }
    }

    fn bridge_remediation_actions(&self) -> Vec<BridgeRemediationAction> {
        self.bridge_remediation
            .lock()
            .map(|engine| engine.actions())
            .unwrap_or_default()
    }

    fn bridge_remediation_dispatches(&self) -> Vec<BridgeRemediationDispatchRecord> {
        let log = bridge_dispatch_log();
        log.lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bridge_ack_latency_observations(&self) -> Vec<(String, String, u64, u64)> {
        self.bridge_remediation
            .lock()
            .map(|engine| {
                engine
                    .ack_latency_observations()
                    .into_iter()
                    .map(|sample| {
                        (
                            sample.playbook.as_str().to_string(),
                            sample.state.as_str().to_string(),
                            sample.latency,
                            sample.count,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[doc(hidden)]
    pub fn bridge_hook_counts(&self) -> (usize, usize, usize, usize) {
        (
            self.bridge_hooks.page.len(),
            self.bridge_hooks.throttle.len(),
            self.bridge_hooks.quarantine.len(),
            self.bridge_hooks.escalate.len(),
        )
    }

    fn record_bridge_dispatch(
        &self,
        action: &BridgeRemediationAction,
        ack: Option<&BridgeDispatchAckRecord>,
        dispatched_at: u64,
        target: &str,
        status: &str,
    ) -> Option<BridgeDispatchUpdate> {
        let (updated, snapshot) = match self.bridge_remediation.lock() {
            Ok(mut engine) => {
                let updated = engine.record_dispatch_attempt(action, ack, dispatched_at, status);
                let snapshot = updated.as_ref().map(|_| engine.snapshot());
                (updated, snapshot)
            }
            Err(_) => {
                warn!(
                    target: "aggregator",
                    "bridge remediation engine poisoned while recording dispatch",
                );
                return None;
            }
        };
        if let Some(snapshot) = snapshot {
            self.persist_bridge_remediation_snapshot(&snapshot);
        }
        if let Some(update) = updated.as_ref() {
            let updated_action = &update.action;
            if let Some(ack) = ack {
                match ack.state {
                    BridgeDispatchAckState::Acknowledged => info!(
                        target: "aggregator",
                        peer = %updated_action.peer_id,
                        metric = %updated_action.metric,
                        action = updated_action.action.as_str(),
                        playbook = updated_action.playbook.as_str(),
                        target,
                        status,
                        timestamp = ack.timestamp,
                        notes = ack.notes.as_deref().unwrap_or(""),
                        "bridge remediation acknowledgement recorded",
                    ),
                    BridgeDispatchAckState::Closed => info!(
                        target: "aggregator",
                        peer = %updated_action.peer_id,
                        metric = %updated_action.metric,
                        action = updated_action.action.as_str(),
                        playbook = updated_action.playbook.as_str(),
                        target,
                        status,
                        timestamp = ack.timestamp,
                        notes = ack.notes.as_deref().unwrap_or(""),
                        "bridge remediation action closed",
                    ),
                    BridgeDispatchAckState::Pending => warn!(
                        target: "aggregator",
                        peer = %updated_action.peer_id,
                        metric = %updated_action.metric,
                        action = updated_action.action.as_str(),
                        playbook = updated_action.playbook.as_str(),
                        target,
                        status,
                        timestamp = ack.timestamp,
                        notes = ack.notes.as_deref().unwrap_or(""),
                        "bridge remediation acknowledgement pending",
                    ),
                    BridgeDispatchAckState::Invalid => warn!(
                        target: "aggregator",
                        peer = %updated_action.peer_id,
                        metric = %updated_action.metric,
                        action = updated_action.action.as_str(),
                        playbook = updated_action.playbook.as_str(),
                        target,
                        status,
                        timestamp = ack.timestamp,
                        notes = ack.notes.as_deref().unwrap_or(""),
                        "bridge remediation acknowledgement invalid",
                    ),
                }
            } else if status == "success" && updated_action.pending_since.is_some() {
                warn!(
                    target: "aggregator",
                    peer = %updated_action.peer_id,
                    metric = %updated_action.metric,
                    action = updated_action.action.as_str(),
                    playbook = updated_action.playbook.as_str(),
                    target,
                    status,
                    attempts = updated_action.dispatch_attempts,
                    "bridge remediation awaiting acknowledgement",
                );
            }
        }
        updated
    }

    fn record_correlation(&self, metric: &str, record: CorrelationRecord) {
        if record.correlation_id.is_empty() {
            return;
        }
        let mut map = self.correlations.lock().unwrap();
        let entry = map.entry(metric.to_string()).or_insert_with(VecDeque::new);
        entry.push_back(record.clone());
        while entry.len() > MAX_CORRELATIONS_PER_METRIC {
            entry.pop_front();
        }
        info!(
            target: "aggregator",
            metric,
            peer = %record.peer_id,
            correlation = %record.correlation_id,
            "indexed metric/log correlation"
        );
    }

    fn correlations_for(&self, metric: &str) -> Vec<CorrelationRecord> {
        self.correlations
            .lock()
            .unwrap()
            .get(metric)
            .map(|deque| deque.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn handle_quic_failure(&self, record: &CorrelationRecord) {
        if record.correlation_id.is_empty() {
            return;
        }
        let Some(value) = record.value else {
            return;
        };
        let mut cache = self.last_metric_values.lock().unwrap();
        let key = (record.peer_id.clone(), record.metric.clone());
        let previous = cache.insert(key, value);
        if let Some(prev) = previous {
            if value <= prev {
                return;
            }
        }
        drop(cache);
        info!(
            target: "aggregator",
            metric = %record.metric,
            peer = %record.peer_id,
            correlation = %record.correlation_id,
            "quic handshake failures increased"
        );
        spawn_log_dump(record.clone());
    }

    fn record_tls_warning_samples(&self, peer_id: &str, metrics: &Value) {
        let counter_samples = extract_tls_warning_counters(metrics);
        let gauge_samples = extract_tls_warning_last_seen(metrics);
        let detail_fingerprints = extract_tls_warning_detail_fingerprints(metrics);
        let variables_fingerprints = extract_tls_warning_variables_fingerprints(metrics);
        if counter_samples.is_empty()
            && gauge_samples.is_empty()
            && detail_fingerprints.is_empty()
            && variables_fingerprints.is_empty()
        {
            return;
        }

        let mut metadata_map: HashMap<(String, String), TlsWarningMetadata> = HashMap::new();
        for sample in detail_fingerprints {
            let prefix = sample.prefix;
            let code = sample.code;
            match sample.value {
                TlsFingerprintValue::Parsed(fingerprint) => {
                    let entry = metadata_map
                        .entry((prefix.clone(), code.clone()))
                        .or_insert_with(|| TlsWarningMetadata::peer(peer_id));
                    entry.detail_fingerprint = Some(fingerprint);
                }
                TlsFingerprintValue::Invalid(raw) => warn!(
                    target: "aggregator",
                    %peer_id,
                    %prefix,
                    %code,
                    value = %raw,
                    "ignored invalid tls warning detail fingerprint sample",
                ),
            }
        }

        for sample in variables_fingerprints {
            let prefix = sample.prefix;
            let code = sample.code;
            match sample.value {
                TlsFingerprintValue::Parsed(fingerprint) => {
                    let entry = metadata_map
                        .entry((prefix.clone(), code.clone()))
                        .or_insert_with(|| TlsWarningMetadata::peer(peer_id));
                    entry.variables_fingerprint = Some(fingerprint);
                }
                TlsFingerprintValue::Invalid(raw) => warn!(
                    target: "aggregator",
                    %peer_id,
                    %prefix,
                    %code,
                    value = %raw,
                    "ignored invalid tls warning variables fingerprint sample",
                ),
            }
        }

        let mut cache = self.tls_warning_counters.lock().unwrap();
        for (prefix, code, value) in counter_samples {
            if !value.is_finite() || value < 0.0 {
                warn!(
                    target: "aggregator",
                    %peer_id,
                    %prefix,
                    %code,
                    value,
                    "ignored non-finite tls warning counter sample",
                );
                continue;
            }

            let key = (peer_id.to_string(), prefix.clone(), code.clone());
            let previous = cache.get(&key).copied();
            let delta_value = match previous {
                Some(prev) if value > prev + COUNTER_EPSILON => value - prev,
                Some(_) => {
                    cache.insert(key, value);
                    continue;
                }
                None => value,
            };
            cache.insert(key, value);

            if let Some(delta) = quantize_counter(delta_value) {
                let metadata = metadata_map
                    .get(&(prefix.clone(), code.clone()))
                    .cloned()
                    .unwrap_or_else(|| TlsWarningMetadata::peer(peer_id));
                record_tls_env_warning_event(&prefix, &code, delta, metadata);
            } else {
                warn!(
                    target: "aggregator",
                    %peer_id,
                    %prefix,
                    %code,
                    delta_value,
                    "unable to quantize tls warning delta",
                );
            }
        }

        drop(cache);

        for (prefix, code, value) in gauge_samples {
            if !value.is_finite() || value < 0.0 {
                warn!(
                    target: "aggregator",
                    %peer_id,
                    %prefix,
                    %code,
                    value,
                    "ignored invalid tls warning last seen sample",
                );
                continue;
            }
            if value == 0.0 {
                continue;
            }

            let timestamp = value.round();
            if timestamp.is_nan() || timestamp.is_infinite() || timestamp < 0.0 {
                continue;
            }
            let metadata = metadata_map
                .get(&(prefix.clone(), code.clone()))
                .cloned()
                .unwrap_or_else(|| TlsWarningMetadata::peer(peer_id));
            record_tls_env_warning_last_seen(&prefix, &code, timestamp as u64, metadata);
        }
    }
    fn record_telemetry(&self, entry: TelemetrySummary) {
        if let Ok(mut map) = self.telemetry.lock() {
            let deque = map
                .entry(entry.node_id.clone())
                .or_insert_with(VecDeque::new);
            deque.push_back(entry);
            while deque.len() > TELEMETRY_WINDOW {
                deque.pop_front();
            }
        }
    }

    fn telemetry_latest(&self) -> HashMap<String, TelemetrySummary> {
        self.telemetry
            .lock()
            .map(|map| {
                map.iter()
                    .filter_map(|(node, deque)| {
                        deque.back().cloned().map(|entry| (node.clone(), entry))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn telemetry_history(&self, node: &str) -> Vec<TelemetrySummary> {
        self.telemetry
            .lock()
            .ok()
            .and_then(|map| map.get(node).cloned())
            .map(|deque| deque.into_iter().collect())
            .unwrap_or_default()
    }

    fn wrappers_latest(&self) -> HashMap<String, WrapperSummaryEntry> {
        self.telemetry
            .lock()
            .map(|map| {
                map.iter()
                    .filter_map(|(node, deque)| {
                        deque
                            .back()
                            .map(|entry| (node.clone(), entry.wrappers.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn store_handle(&self) -> Arc<InhouseEngine> {
        Arc::clone(&self.store)
    }

    pub(crate) fn db_path(&self) -> Arc<PathBuf> {
        Arc::clone(&self.db_path)
    }

    pub(crate) fn update_leader_state(
        &self,
        is_leader: bool,
        leader_id: Option<String>,
        fencing: u64,
    ) {
        self.leader_flag.store(is_leader, Ordering::SeqCst);
        self.leader_fencing.store(fencing, Ordering::SeqCst);
        if let Ok(mut guard) = self.leader_id.write() {
            *guard = leader_id;
        }
    }

    pub fn is_leader(&self) -> bool {
        self.leader_flag.load(Ordering::SeqCst)
    }

    pub fn leader_snapshot(&self) -> LeaderSnapshot {
        let leader_id = self
            .leader_id
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        LeaderSnapshot {
            is_leader: self.is_leader(),
            leader_id,
            fencing_token: self.leader_fencing.load(Ordering::SeqCst),
        }
    }
}

struct AggregatorMetrics {
    registry: Registry,
    ingest_total: Counter,
    bulk_export_total: Counter,
    active_peers: Gauge,
    replication_lag: Gauge,
    runtime_spawn_latency: Histogram,
    runtime_pending_tasks: Gauge,
    retention_pruned_total: Counter,
    telemetry_ingest_total: Counter,
    telemetry_schema_error_total: Counter,
    tls_env_warning_total: CounterVec,
    tls_env_warning_events_total: CounterVec,
    tls_env_warning_last_seen: GaugeVec,
    tls_env_warning_retention_seconds: Gauge,
    tls_env_warning_active_snapshots: Gauge,
    tls_env_warning_stale_snapshots: Gauge,
    tls_env_warning_most_recent_last_seen: Gauge,
    tls_env_warning_least_recent_last_seen: Gauge,
    tls_env_warning_detail_fingerprint: IntGaugeVec,
    tls_env_warning_variables_fingerprint: IntGaugeVec,
    tls_env_warning_detail_fingerprint_total: CounterVec,
    tls_env_warning_variables_fingerprint_total: CounterVec,
    tls_env_warning_detail_unique_fingerprints: IntGaugeVec,
    tls_env_warning_variables_unique_fingerprints: IntGaugeVec,
    treasury_disbursement_count: GaugeVec,
    treasury_disbursement_amount: GaugeVec,
    treasury_disbursement_snapshot_age: Gauge,
    treasury_disbursement_scheduled_oldest_age: Gauge,
    treasury_disbursement_next_epoch: Gauge,
    treasury_balance_current: Gauge,
    treasury_balance_last_delta: Gauge,
    treasury_balance_snapshot_count: Gauge,
    treasury_balance_last_event_age: Gauge,
    _bridge_anomaly_total: Counter,
    bridge_metric_delta: GaugeVec,
    bridge_metric_rate_per_second: GaugeVec,
    bridge_remediation_action_total: CounterVec,
    bridge_remediation_dispatch_total: CounterVec,
    bridge_remediation_dispatch_ack_total: CounterVec,
    bridge_remediation_ack_target_seconds: GaugeVec,
    bridge_remediation_ack_latency_seconds: HistogramVec,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct TlsWarningSnapshot {
    prefix: String,
    code: String,
    total: u64,
    last_delta: u64,
    last_seen: u64,
    origin: WarningOrigin,
    peer_id: Option<String>,
    detail: Option<String>,
    variables: Vec<String>,
    detail_fingerprint: Option<i64>,
    variables_fingerprint: Option<i64>,
    detail_fingerprint_counts: BTreeMap<String, u64>,
    variables_fingerprint_counts: BTreeMap<String, u64>,
}

#[derive(Clone)]
struct TlsWarningUpdate {
    last_seen: u64,
    detail_fingerprint: Option<i64>,
    variables_fingerprint: Option<i64>,
    detail_bucket: String,
    variables_bucket: String,
    detail_unique: usize,
    variables_unique: usize,
    detail_new: bool,
    variables_new: bool,
}

impl TlsWarningSnapshot {
    fn new(prefix: &str, code: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            code: code.to_string(),
            total: 0,
            last_delta: 0,
            last_seen: 0,
            origin: WarningOrigin::PeerIngest,
            peer_id: None,
            detail: None,
            variables: Vec::new(),
            detail_fingerprint: None,
            variables_fingerprint: None,
            detail_fingerprint_counts: BTreeMap::new(),
            variables_fingerprint_counts: BTreeMap::new(),
        }
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("prefix".to_string(), Value::String(self.prefix.clone()));
        map.insert("code".to_string(), Value::String(self.code.clone()));
        map.insert("total".to_string(), Value::from(self.total));
        map.insert("last_delta".to_string(), Value::from(self.last_delta));
        map.insert("last_seen".to_string(), Value::from(self.last_seen));
        map.insert(
            "origin".to_string(),
            Value::String(self.origin.as_str().into()),
        );
        map.insert(
            "peer_id".to_string(),
            self.peer_id
                .as_ref()
                .map(|value| Value::String(value.clone()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "detail".to_string(),
            self.detail
                .as_ref()
                .map(|value| Value::String(value.clone()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "variables".to_string(),
            Value::Array(
                self.variables
                    .iter()
                    .map(|value| Value::String(value.clone()))
                    .collect(),
            ),
        );
        map.insert(
            "detail_fingerprint".to_string(),
            self.detail_fingerprint
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "variables_fingerprint".to_string(),
            self.variables_fingerprint
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "detail_fingerprint_counts".to_string(),
            map_from_counts(&self.detail_fingerprint_counts),
        );
        map.insert(
            "variables_fingerprint_counts".to_string(),
            map_from_counts(&self.variables_fingerprint_counts),
        );
        Value::Object(map)
    }
}

fn map_from_counts(counts: &BTreeMap<String, u64>) -> Value {
    let mut map = Map::new();
    for (key, value) in counts {
        map.insert(key.clone(), Value::from(*value));
    }
    Value::Object(map)
}

fn memory_snapshot_to_value(entry: &MemorySnapshotEntry) -> Value {
    let mut map = Map::new();
    map.insert("latest".to_string(), Value::from(entry.latest));
    map.insert("p50".to_string(), Value::from(entry.p50));
    map.insert("p90".to_string(), Value::from(entry.p90));
    map.insert("p99".to_string(), Value::from(entry.p99));
    Value::Object(map)
}

fn wrapper_metric_to_value(entry: &WrapperMetricEntry) -> Value {
    let mut labels = Map::new();
    let mut keys: Vec<_> = entry.labels.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if let Some(value) = entry.labels.get(&key) {
            labels.insert(key, Value::String(value.clone()));
        }
    }
    let mut map = Map::new();
    map.insert("metric".to_string(), Value::String(entry.metric.clone()));
    map.insert("labels".to_string(), Value::Object(labels));
    map.insert("value".to_string(), Value::from(entry.value));
    Value::Object(map)
}

fn wrapper_summary_to_value(summary: &WrapperSummaryEntry) -> Value {
    let metrics = summary
        .metrics
        .iter()
        .map(wrapper_metric_to_value)
        .collect();
    let mut map = Map::new();
    map.insert("metrics".to_string(), Value::Array(metrics));
    Value::Object(map)
}

fn wrappers_map_to_value(map: &HashMap<String, WrapperSummaryEntry>) -> Value {
    let mut object = Map::new();
    let mut keys: Vec<_> = map.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if let Some(summary) = map.get(&key) {
            object.insert(key, wrapper_summary_to_value(summary));
        }
    }
    Value::Object(object)
}

fn telemetry_summary_to_value(summary: &TelemetrySummary) -> Value {
    let mut map = Map::new();
    map.insert(
        "node_id".to_string(),
        Value::String(summary.node_id.clone()),
    );
    map.insert("seq".to_string(), Value::from(summary.seq));
    map.insert("timestamp".to_string(), Value::from(summary.timestamp));
    map.insert(
        "sample_rate_ppm".to_string(),
        Value::from(summary.sample_rate_ppm),
    );
    map.insert(
        "compaction_secs".to_string(),
        Value::from(summary.compaction_secs),
    );
    let mut memory_map = Map::new();
    let mut buckets: Vec<_> = summary.memory.keys().cloned().collect();
    buckets.sort();
    for bucket in buckets {
        if let Some(entry) = summary.memory.get(&bucket) {
            memory_map.insert(bucket, memory_snapshot_to_value(entry));
        }
    }
    map.insert("memory".to_string(), Value::Object(memory_map));
    map.insert(
        "wrappers".to_string(),
        wrapper_summary_to_value(&summary.wrappers),
    );
    Value::Object(map)
}

fn telemetry_summary_map_to_value(map: &HashMap<String, TelemetrySummary>) -> Value {
    let mut object = Map::new();
    let mut keys: Vec<_> = map.keys().cloned().collect();
    keys.sort();
    for key in keys {
        if let Some(summary) = map.get(&key) {
            object.insert(key, telemetry_summary_to_value(summary));
        }
    }
    Value::Object(object)
}

fn telemetry_history_to_value(history: &[TelemetrySummary]) -> Value {
    let entries = history.iter().map(telemetry_summary_to_value).collect();
    Value::Array(entries)
}

fn telemetry_summary_from_value(value: &Value) -> Result<TelemetrySummary, ValidationError> {
    TelemetrySummary::validate_value(value)?;
    let object = value
        .as_object()
        .expect("validated telemetry summary must be an object");
    let node_id = object
        .get("node_id")
        .and_then(Value::as_str)
        .expect("validated telemetry summary has node_id")
        .to_string();
    let seq = object
        .get("seq")
        .and_then(Value::as_u64)
        .expect("validated telemetry summary has seq");
    let timestamp = object
        .get("timestamp")
        .and_then(Value::as_u64)
        .expect("validated telemetry summary has timestamp");
    let sample_rate_ppm = object
        .get("sample_rate_ppm")
        .and_then(Value::as_u64)
        .expect("validated telemetry summary has sample_rate_ppm");
    let compaction_secs = object
        .get("compaction_secs")
        .and_then(Value::as_u64)
        .expect("validated telemetry summary has compaction_secs");

    let memory_value = object
        .get("memory")
        .and_then(Value::as_object)
        .expect("validated telemetry summary has memory");
    let mut memory = HashMap::new();
    for (bucket, entry_value) in memory_value {
        let entry = entry_value
            .as_object()
            .expect("validated telemetry memory entry must be object");
        let latest = entry
            .get("latest")
            .and_then(Value::as_u64)
            .expect("memory entry latest");
        let p50 = entry
            .get("p50")
            .and_then(Value::as_u64)
            .expect("memory entry p50");
        let p90 = entry
            .get("p90")
            .and_then(Value::as_u64)
            .expect("memory entry p90");
        let p99 = entry
            .get("p99")
            .and_then(Value::as_u64)
            .expect("memory entry p99");
        memory.insert(
            bucket.clone(),
            MemorySnapshotEntry {
                latest,
                p50,
                p90,
                p99,
            },
        );
    }

    let metrics = object
        .get("wrappers")
        .and_then(Value::as_object)
        .and_then(|wrapper| wrapper.get("metrics").and_then(Value::as_array))
        .cloned()
        .unwrap_or_else(Vec::new);
    let mut wrapper_metrics = Vec::with_capacity(metrics.len());
    for metric_value in metrics {
        let metric_obj = metric_value
            .as_object()
            .expect("validated wrapper metric must be object");
        let metric_name = metric_obj
            .get("metric")
            .and_then(Value::as_str)
            .expect("wrapper metric name")
            .to_string();
        let value = metric_obj
            .get("value")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let labels = metric_obj
            .get("labels")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_str().map(|value| (k.clone(), value.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        wrapper_metrics.push(WrapperMetricEntry {
            metric: metric_name,
            labels,
            value,
        });
    }

    Ok(TelemetrySummary {
        node_id,
        seq,
        timestamp,
        sample_rate_ppm,
        compaction_secs,
        memory,
        wrappers: WrapperSummaryEntry {
            metrics: wrapper_metrics,
        },
    })
}

#[derive(Clone)]
struct TlsWarningMetadata {
    detail: Option<String>,
    variables: Vec<String>,
    origin: WarningOrigin,
    peer_id: Option<String>,
    detail_fingerprint: Option<i64>,
    variables_fingerprint: Option<i64>,
}

#[derive(Clone, Debug)]
struct TlsFingerprintSample {
    prefix: String,
    code: String,
    value: TlsFingerprintValue,
}

#[derive(Clone, Debug)]
enum TlsFingerprintValue {
    Parsed(i64),
    Invalid(String),
}

impl TlsWarningMetadata {
    fn diagnostics(detail: String, variables: Vec<String>) -> Self {
        let detail = if detail.is_empty() {
            None
        } else {
            Some(detail)
        };
        let detail_fingerprint = detail
            .as_ref()
            .map(|value| tls_detail_fingerprint(value.as_str()));
        let variables_fingerprint =
            tls_variables_fingerprint(variables.iter().map(|value| value.as_str()));
        Self {
            detail,
            variables,
            origin: WarningOrigin::Diagnostics,
            peer_id: None,
            detail_fingerprint,
            variables_fingerprint,
        }
    }

    fn peer(peer_id: &str) -> Self {
        Self {
            detail: None,
            variables: Vec::new(),
            origin: WarningOrigin::PeerIngest,
            peer_id: Some(peer_id.to_string()),
            detail_fingerprint: None,
            variables_fingerprint: None,
        }
    }

    fn resolved_detail_fingerprint(&self) -> Option<i64> {
        match self.detail_fingerprint {
            Some(0) => None,
            Some(value) => Some(value),
            None => self
                .detail
                .as_ref()
                .map(|value| tls_detail_fingerprint(value.as_str())),
        }
    }

    fn resolved_variables_fingerprint(&self) -> Option<i64> {
        match self.variables_fingerprint {
            Some(0) => None,
            Some(value) => Some(value),
            None => tls_variables_fingerprint(self.variables.iter().map(|value| value.as_str())),
        }
    }
}

impl Default for TlsWarningMetadata {
    fn default() -> Self {
        Self {
            detail: None,
            variables: Vec::new(),
            origin: WarningOrigin::PeerIngest,
            peer_id: None,
            detail_fingerprint: None,
            variables_fingerprint: None,
        }
    }
}

impl AggregatorMetrics {
    fn registry(&self) -> &Registry {
        &self.registry
    }
}

#[derive(Clone)]
struct AggregatorRecorder {
    metrics: &'static AggregatorMetrics,
}

impl AggregatorRecorder {
    fn new() -> Self {
        Self {
            metrics: aggregator_metrics(),
        }
    }

    fn u64_delta(metric: &str, value: f64) -> Option<u64> {
        if !value.is_finite() {
            warn!(target: "aggregator", metric, %value, "discarding non-finite counter delta");
            return None;
        }
        if value < 0.0 {
            warn!(
                target: "aggregator",
                metric,
                %value,
                "discarding negative counter delta"
            );
            return None;
        }
        Some(value.round() as u64)
    }

    fn i64_value(metric: &str, value: f64) -> Option<i64> {
        if !value.is_finite() {
            warn!(target: "aggregator", metric, %value, "discarding non-finite gauge value");
            return None;
        }
        Some(value.round() as i64)
    }

    fn f64_value(metric: &str, value: f64) -> Option<f64> {
        if !value.is_finite() {
            warn!(target: "aggregator", metric, %value, "discarding non-finite gauge value");
            return None;
        }
        Some(value)
    }

    fn label_values<'a>(
        metric: &str,
        labels: &'a [(String, String)],
        expected: &[&str],
    ) -> Option<Vec<&'a str>> {
        if labels.len() != expected.len() {
            warn!(
                target: "aggregator",
                metric,
                expected = %expected.join(","),
                actual = labels.len(),
                "unexpected label cardinality"
            );
            return None;
        }
        let mut values = Vec::with_capacity(expected.len());
        for ((key, value), expected_key) in labels.iter().zip(expected.iter()) {
            if key != expected_key {
                warn!(
                    target: "aggregator",
                    metric,
                    expected = *expected_key,
                    actual = key.as_str(),
                    "unexpected label key"
                );
                return None;
            }
            values.push(value.as_str());
        }
        Some(values)
    }
}

impl Default for AggregatorRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder for AggregatorRecorder {
    fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let metrics = self.metrics;
        match name {
            METRIC_AGGREGATOR_INGEST_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    metrics.ingest_total.inc_by(delta);
                }
            }
            METRIC_BULK_EXPORT_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    metrics.bulk_export_total.inc_by(delta);
                }
            }
            METRIC_AGGREGATOR_RETENTION_PRUNED_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    metrics.retention_pruned_total.inc_by(delta);
                }
            }
            METRIC_TELEMETRY_INGEST_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    metrics.telemetry_ingest_total.inc_by(delta);
                }
            }
            METRIC_TELEMETRY_SCHEMA_ERROR_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    metrics.telemetry_schema_error_total.inc_by(delta);
                }
            }
            METRIC_TLS_ENV_WARNING_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_total
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.inc_by(delta),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_total"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_EVENTS_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    if let Some(values) =
                        Self::label_values(name, labels, &LABEL_PREFIX_CODE_ORIGIN)
                    {
                        match metrics
                            .tls_env_warning_events_total
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.inc_by(delta),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_events_total"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_DETAIL_FINGERPRINT_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    if let Some(values) =
                        Self::label_values(name, labels, &LABEL_PREFIX_CODE_FINGERPRINT)
                    {
                        match metrics
                            .tls_env_warning_detail_fingerprint_total
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.inc_by(delta),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_detail_fingerprint_total"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_VARIABLES_FINGERPRINT_TOTAL => {
                if let Some(delta) = Self::u64_delta(name, value) {
                    if let Some(values) =
                        Self::label_values(name, labels, &LABEL_PREFIX_CODE_FINGERPRINT)
                    {
                        match metrics
                            .tls_env_warning_variables_fingerprint_total
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.inc_by(delta),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_variables_fingerprint_total"
                            ),
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn record_histogram(&self, name: &str, value: f64, labels: &[(String, String)]) {
        if !labels.is_empty() {
            warn!(
                target: "aggregator",
                metric = name,
                label_count = labels.len(),
                "histogram metrics do not accept labels"
            );
            return;
        }
        if name == METRIC_RUNTIME_SPAWN_LATENCY {
            if let Some(sample) = Self::f64_value(name, value) {
                self.metrics.runtime_spawn_latency.observe(sample);
            }
        }
    }

    fn record_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let metrics = self.metrics;
        match name {
            METRIC_CLUSTER_PEER_ACTIVE_TOTAL => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.active_peers.set(sample);
                }
            }
            METRIC_AGGREGATOR_REPLICATION_LAG => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.replication_lag.set(sample);
                }
            }
            METRIC_RUNTIME_PENDING_TASKS => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.runtime_pending_tasks.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_RETENTION_SECONDS => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.tls_env_warning_retention_seconds.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_ACTIVE_SNAPSHOTS => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.tls_env_warning_active_snapshots.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_STALE_SNAPSHOTS => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.tls_env_warning_stale_snapshots.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_MOST_RECENT_LAST_SEEN => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.tls_env_warning_most_recent_last_seen.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_LEAST_RECENT_LAST_SEEN => {
                if let Some(sample) = Self::f64_value(name, value) {
                    metrics.tls_env_warning_least_recent_last_seen.set(sample);
                }
            }
            METRIC_TLS_ENV_WARNING_LAST_SEEN => {
                if let Some(sample) = Self::f64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_last_seen
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_last_seen_seconds"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_DETAIL_FINGERPRINT => {
                if let Some(sample) = Self::i64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_detail_fingerprint
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_detail_fingerprint"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_VARIABLES_FINGERPRINT => {
                if let Some(sample) = Self::i64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_variables_fingerprint
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_variables_fingerprint"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_DETAIL_UNIQUE_FINGERPRINTS => {
                if let Some(sample) = Self::i64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_detail_unique_fingerprints
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_detail_unique_fingerprints"
                            ),
                        }
                    }
                }
            }
            METRIC_TLS_ENV_WARNING_VARIABLES_UNIQUE_FINGERPRINTS => {
                if let Some(sample) = Self::i64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_PREFIX_CODE) {
                        match metrics
                            .tls_env_warning_variables_unique_fingerprints
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update tls_env_warning_variables_unique_fingerprints"
                            ),
                        }
                    }
                }
            }
            METRIC_BRIDGE_COUNTER_DELTA => {
                if let Some(sample) = Self::f64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_BRIDGE_COUNTER) {
                        match metrics
                            .bridge_metric_delta
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update bridge_metric_delta"
                            ),
                        }
                    }
                }
            }
            METRIC_BRIDGE_COUNTER_RATE => {
                if let Some(sample) = Self::f64_value(name, value) {
                    if let Some(values) = Self::label_values(name, labels, &LABEL_BRIDGE_COUNTER) {
                        match metrics
                            .bridge_metric_rate_per_second
                            .ensure_handle_for_label_values(&values)
                        {
                            Ok(handle) => handle.set(sample),
                            Err(err) => warn!(
                                target: "aggregator",
                                metric = name,
                                ?err,
                                "failed to update bridge_metric_rate_per_second"
                            ),
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

static FOUNDATION_METRICS_RECORDER_GUARD: OnceLock<()> = OnceLock::new();

pub fn install_foundation_metrics_recorder() -> Result<(), RecorderInstallError> {
    foundation_metrics::install_recorder(AggregatorRecorder::new())
}

pub fn ensure_foundation_metrics_recorder() {
    FOUNDATION_METRICS_RECORDER_GUARD.get_or_init(|| {
        if let Err(err) = install_foundation_metrics_recorder() {
            warn!(
                target: "aggregator",
                error = %err,
                "failed to install foundation metrics recorder"
            );
        }
    });
}

static METRICS: Lazy<AggregatorMetrics> = Lazy::new(|| {
    let registry = Registry::new();
    let ingest_total = registry
        .register_counter("aggregator_ingest_total", "Total peer metric ingests")
        .expect("register aggregator_ingest_total");
    let bulk_export_total = registry
        .register_counter("bulk_export_total", "Total bulk export attempts")
        .expect("register bulk_export_total");
    let active_peers = registry
        .register_gauge(
            "cluster_peer_active_total",
            "Unique peers tracked by aggregator",
        )
        .expect("register cluster_peer_active_total");
    let replication_lag = registry
        .register_gauge(
            "aggregator_replication_lag_seconds",
            "Seconds since last WAL entry applied",
        )
        .expect("register aggregator_replication_lag_seconds");
    let runtime_spawn_latency = Histogram::with_opts(HistogramOpts::new(
        METRIC_RUNTIME_SPAWN_LATENCY,
        "Runtime task spawn latency observed inside metrics-aggregator",
    ))
    .expect("build runtime_spawn_latency histogram");
    registry
        .register(Box::new(runtime_spawn_latency.clone()))
        .expect("register runtime_spawn_latency_seconds");
    let runtime_pending_tasks = registry
        .register_gauge(
            METRIC_RUNTIME_PENDING_TASKS,
            "Runtime pending-task gauge for metrics-aggregator",
        )
        .expect("register runtime_pending_tasks");
    let retention_pruned_total = registry
        .register_counter(
            "aggregator_retention_pruned_total",
            "Peer metric samples pruned by retention",
        )
        .expect("register aggregator_retention_pruned_total");
    let telemetry_ingest_total = registry
        .register_counter(
            "aggregator_telemetry_ingest_total",
            "Telemetry summaries accepted by schema guard",
        )
        .expect("register aggregator_telemetry_ingest_total");
    let telemetry_schema_error_total = registry
        .register_counter(
            "aggregator_telemetry_schema_error_total",
            "Telemetry payloads rejected due to schema drift",
        )
        .expect("register aggregator_telemetry_schema_error_total");
    let tls_env_warning_total = CounterVec::new(
        Opts::new(
            "tls_env_warning_total",
            "TLS environment configuration warnings grouped by prefix and code",
        ),
        &["prefix", "code"],
    )
    .expect("build tls_env_warning_total counter vec");
    registry
        .register(Box::new(tls_env_warning_total.clone()))
        .expect("register tls_env_warning_total");
    let tls_env_warning_events_total = CounterVec::new(
        Opts::new(
            "tls_env_warning_events_total",
            "TLS warning events grouped by prefix, code, and origin",
        ),
        &["prefix", "code", "origin"],
    )
    .expect("build tls_env_warning_events_total counter vec");
    registry
        .register(Box::new(tls_env_warning_events_total.clone()))
        .expect("register tls_env_warning_events_total");
    let tls_env_warning_last_seen = GaugeVec::new(
        Opts::new(
            "tls_env_warning_last_seen_seconds",
            "Unix timestamp of the most recent TLS environment warning",
        ),
        &["prefix", "code"],
    );
    registry
        .register(Box::new(tls_env_warning_last_seen.clone()))
        .expect("register tls_env_warning_last_seen");
    let tls_env_warning_retention_seconds = registry
        .register_gauge(
            "tls_env_warning_retention_seconds",
            "Retention window for TLS warning snapshots in seconds",
        )
        .expect("register tls_env_warning_retention_seconds");
    let tls_env_warning_active_snapshots = registry
        .register_gauge(
            "tls_env_warning_active_snapshots",
            "Number of active TLS warning snapshots tracked by the aggregator",
        )
        .expect("register tls_env_warning_active_snapshots");
    let tls_env_warning_stale_snapshots = registry
        .register_gauge(
            "tls_env_warning_stale_snapshots",
            "TLS warning snapshots older than the configured retention window",
        )
        .expect("register tls_env_warning_stale_snapshots");
    let tls_env_warning_most_recent_last_seen = registry
        .register_gauge(
            "tls_env_warning_most_recent_last_seen_seconds",
            "Last-seen timestamp of the most recent TLS warning snapshot",
        )
        .expect("register tls_env_warning_most_recent_last_seen_seconds");
    let tls_env_warning_least_recent_last_seen = registry
        .register_gauge(
            "tls_env_warning_least_recent_last_seen_seconds",
            "Last-seen timestamp of the stalest TLS warning snapshot",
        )
        .expect("register tls_env_warning_least_recent_last_seen_seconds");
    let tls_env_warning_detail_fingerprint = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_detail_fingerprint",
            "Fingerprint of the most recent TLS warning detail payload",
        ),
        &["prefix", "code"],
    )
    .expect("build tls_env_warning_detail_fingerprint gauge vec");
    registry
        .register(Box::new(tls_env_warning_detail_fingerprint.clone()))
        .expect("register tls_env_warning_detail_fingerprint");
    let tls_env_warning_variables_fingerprint = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_variables_fingerprint",
            "Fingerprint of the most recent TLS warning variable payload",
        ),
        &["prefix", "code"],
    )
    .expect("build tls_env_warning_variables_fingerprint gauge vec");
    registry
        .register(Box::new(tls_env_warning_variables_fingerprint.clone()))
        .expect("register tls_env_warning_variables_fingerprint");
    let tls_env_warning_detail_unique_fingerprints = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_detail_unique_fingerprints",
            "Unique TLS warning detail fingerprints observed",
        ),
        &["prefix", "code"],
    )
    .expect("build tls_env_warning_detail_unique_fingerprints gauge vec");
    registry
        .register(Box::new(tls_env_warning_detail_unique_fingerprints.clone()))
        .expect("register tls_env_warning_detail_unique_fingerprints");
    let tls_env_warning_variables_unique_fingerprints = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_variables_unique_fingerprints",
            "Unique TLS warning variables fingerprints observed",
        ),
        &["prefix", "code"],
    )
    .expect("build tls_env_warning_variables_unique_fingerprints gauge vec");
    registry
        .register(Box::new(
            tls_env_warning_variables_unique_fingerprints.clone(),
        ))
        .expect("register tls_env_warning_variables_unique_fingerprints");
    let tls_env_warning_detail_fingerprint_total = CounterVec::new(
        Opts::new(
            "tls_env_warning_detail_fingerprint_total",
            "Cumulative TLS warning events grouped by detail fingerprint",
        ),
        &["prefix", "code", "fingerprint"],
    )
    .expect("build tls_env_warning_detail_fingerprint_total counter vec");
    registry
        .register(Box::new(tls_env_warning_detail_fingerprint_total.clone()))
        .expect("register tls_env_warning_detail_fingerprint_total");
    let tls_env_warning_variables_fingerprint_total = CounterVec::new(
        Opts::new(
            "tls_env_warning_variables_fingerprint_total",
            "Cumulative TLS warning events grouped by variables fingerprint",
        ),
        &["prefix", "code", "fingerprint"],
    )
    .expect("build tls_env_warning_variables_fingerprint_total counter vec");
    registry
        .register(Box::new(
            tls_env_warning_variables_fingerprint_total.clone(),
        ))
        .expect("register tls_env_warning_variables_fingerprint_total");
    let _bridge_anomaly_total = registry
        .register_counter(
            METRIC_BRIDGE_ANOMALY_TOTAL,
            "Bridge anomaly alerts emitted by the aggregator",
        )
        .expect("register bridge_anomaly_total");
    let treasury_disbursement_count = GaugeVec::new(
        Opts::new(
            METRIC_TREASURY_COUNT,
            "Treasury disbursement counts grouped by status",
        ),
        &["status"],
    );
    registry
        .register(Box::new(treasury_disbursement_count.clone()))
        .expect("register treasury_disbursement_count");
    let treasury_disbursement_amount = GaugeVec::new(
        Opts::new(
            METRIC_TREASURY_AMOUNT_CT,
            "Treasury disbursement CT totals grouped by status",
        ),
        &["status"],
    );
    registry
        .register(Box::new(treasury_disbursement_amount.clone()))
        .expect("register treasury_disbursement_amount");
    let treasury_disbursement_snapshot_age = Gauge::new(
        METRIC_TREASURY_SNAPSHOT_AGE,
        "Seconds since the most recent treasury disbursement snapshot",
    );
    registry
        .register(Box::new(treasury_disbursement_snapshot_age.clone()))
        .expect("register treasury_disbursement_snapshot_age");
    let treasury_disbursement_scheduled_oldest_age = Gauge::new(
        METRIC_TREASURY_SCHEDULED_OLDEST_AGE,
        "Age in seconds of the oldest scheduled treasury disbursement",
    );
    registry
        .register(Box::new(treasury_disbursement_scheduled_oldest_age.clone()))
        .expect("register treasury_disbursement_scheduled_oldest_age");
    let treasury_disbursement_next_epoch = Gauge::new(
        METRIC_TREASURY_NEXT_EPOCH,
        "Next scheduled treasury disbursement epoch (0 when none queued)",
    );
    registry
        .register(Box::new(treasury_disbursement_next_epoch.clone()))
        .expect("register treasury_disbursement_next_epoch");
    let treasury_balance_current = Gauge::new(
        METRIC_TREASURY_BALANCE_CURRENT,
        "Current treasury balance in CT",
    );
    registry
        .register(Box::new(treasury_balance_current.clone()))
        .expect("register treasury_balance_current");
    let treasury_balance_last_delta = Gauge::new(
        METRIC_TREASURY_BALANCE_LAST_DELTA,
        "Most recent treasury balance delta in CT",
    );
    registry
        .register(Box::new(treasury_balance_last_delta.clone()))
        .expect("register treasury_balance_last_delta");
    let treasury_balance_snapshot_count = Gauge::new(
        METRIC_TREASURY_BALANCE_SNAPSHOT_COUNT,
        "Number of treasury balance snapshots recorded",
    );
    registry
        .register(Box::new(treasury_balance_snapshot_count.clone()))
        .expect("register treasury_balance_snapshot_count");
    let treasury_balance_last_event_age = Gauge::new(
        METRIC_TREASURY_BALANCE_EVENT_AGE,
        "Seconds since the latest treasury balance snapshot was recorded",
    );
    registry
        .register(Box::new(treasury_balance_last_event_age.clone()))
        .expect("register treasury_balance_last_event_age");
    let bridge_metric_delta = GaugeVec::new(
        Opts::new(
            METRIC_BRIDGE_COUNTER_DELTA,
            "Per-scrape bridge counter delta grouped by metric",
        ),
        &["metric", "peer", "labels"],
    );
    registry
        .register(Box::new(bridge_metric_delta.clone()))
        .expect("register bridge_metric_delta");
    let bridge_metric_rate_per_second = GaugeVec::new(
        Opts::new(
            METRIC_BRIDGE_COUNTER_RATE,
            "Per-second bridge counter growth grouped by metric",
        ),
        &["metric", "peer", "labels"],
    );
    registry
        .register(Box::new(bridge_metric_rate_per_second.clone()))
        .expect("register bridge_metric_rate_per_second");
    let bridge_remediation_action_total = CounterVec::new(
        Opts::new(
            METRIC_BRIDGE_REMEDIATION_ACTION_TOTAL,
            "Bridge remediation actions grouped by outcome",
        ),
        &LABEL_REMEDIATION_ACTION,
    )
    .expect("build bridge_remediation_action_total counter vec");
    registry
        .register(Box::new(bridge_remediation_action_total.clone()))
        .expect("register bridge_remediation_action_total");
    let bridge_remediation_dispatch_total = CounterVec::new(
        Opts::new(
            METRIC_BRIDGE_REMEDIATION_DISPATCH_TOTAL,
            "Bridge remediation dispatch attempts grouped by target and status",
        ),
        &LABEL_REMEDIATION_DISPATCH,
    )
    .expect("build bridge_remediation_dispatch_total counter vec");
    registry
        .register(Box::new(bridge_remediation_dispatch_total.clone()))
        .expect("register bridge_remediation_dispatch_total");
    let bridge_remediation_dispatch_ack_total = CounterVec::new(
        Opts::new(
            METRIC_BRIDGE_REMEDIATION_DISPATCH_ACK_TOTAL,
            "Bridge remediation dispatch acknowledgements grouped by target and state",
        ),
        &LABEL_REMEDIATION_ACK,
    )
    .expect("build bridge_remediation_dispatch_ack_total counter vec");
    registry
        .register(Box::new(bridge_remediation_dispatch_ack_total.clone()))
        .expect("register bridge_remediation_dispatch_ack_total");
    let bridge_remediation_ack_target_seconds = GaugeVec::new(
        Opts::new(
            METRIC_BRIDGE_REMEDIATION_ACK_TARGET_SECONDS,
            "Bridge remediation acknowledgement policy targets in seconds",
        ),
        &LABEL_REMEDIATION_ACK_TARGET,
    );
    registry
        .register(Box::new(bridge_remediation_ack_target_seconds.clone()))
        .expect("register bridge_remediation_ack_target_seconds");
    let bridge_remediation_ack_latency_seconds = HistogramVec::new(
        HistogramOpts::new(
            METRIC_BRIDGE_REMEDIATION_ACK_LATENCY_SECONDS,
            "Bridge remediation acknowledgement latency grouped by playbook and state",
        )
        .buckets(vec![
            30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1_800.0, 3_600.0, 7_200.0,
        ]),
        &["playbook", "state"],
    )
    .expect("build bridge_remediation_ack_latency_seconds histogram vec");
    registry
        .register(Box::new(bridge_remediation_ack_latency_seconds.clone()))
        .expect("register bridge_remediation_ack_latency_seconds");
    AggregatorMetrics {
        registry,
        ingest_total,
        bulk_export_total,
        active_peers,
        replication_lag,
        runtime_spawn_latency,
        runtime_pending_tasks,
        retention_pruned_total,
        telemetry_ingest_total,
        telemetry_schema_error_total,
        tls_env_warning_total,
        tls_env_warning_events_total,
        tls_env_warning_last_seen,
        tls_env_warning_retention_seconds,
        tls_env_warning_active_snapshots,
        tls_env_warning_stale_snapshots,
        tls_env_warning_most_recent_last_seen,
        tls_env_warning_least_recent_last_seen,
        tls_env_warning_detail_fingerprint,
        tls_env_warning_variables_fingerprint,
        tls_env_warning_detail_fingerprint_total,
        tls_env_warning_variables_fingerprint_total,
        tls_env_warning_detail_unique_fingerprints,
        tls_env_warning_variables_unique_fingerprints,
        treasury_disbursement_count,
        treasury_disbursement_amount,
        treasury_disbursement_snapshot_age,
        treasury_disbursement_scheduled_oldest_age,
        treasury_disbursement_next_epoch,
        treasury_balance_current,
        treasury_balance_last_delta,
        treasury_balance_snapshot_count,
        treasury_balance_last_event_age,
        _bridge_anomaly_total,
        bridge_metric_delta,
        bridge_metric_rate_per_second,
        bridge_remediation_action_total,
        bridge_remediation_dispatch_total,
        bridge_remediation_dispatch_ack_total,
        bridge_remediation_ack_target_seconds,
        bridge_remediation_ack_latency_seconds,
    }
});

fn aggregator_metrics() -> &'static AggregatorMetrics {
    Lazy::force(&METRICS)
}

static TLS_WARNING_SNAPSHOTS: Lazy<Mutex<HashMap<(String, String), TlsWarningSnapshot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static BRIDGE_DISPATCH_LOG: OnceLock<Arc<Mutex<VecDeque<BridgeRemediationDispatchRecord>>>> =
    OnceLock::new();

fn bridge_dispatch_log() -> &'static Arc<Mutex<VecDeque<BridgeRemediationDispatchRecord>>> {
    BRIDGE_DISPATCH_LOG.get_or_init(|| Arc::new(Mutex::new(VecDeque::new())))
}

pub fn reset_bridge_remediation_dispatch_log() {
    if let Some(log) = BRIDGE_DISPATCH_LOG.get() {
        if let Ok(mut guard) = log.lock() {
            guard.clear();
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn reset_bridge_remediation_ack_metrics() {
    let metrics = aggregator_metrics();
    for playbook in BridgeRemediationPlaybook::variants() {
        for state in BridgeDispatchAckState::variants() {
            let _ = metrics
                .bridge_remediation_ack_latency_seconds
                .remove_label_values(&[playbook.as_str(), state.as_str()]);
        }
        for phase in ["retry", "escalate"] {
            let _ = metrics
                .bridge_remediation_ack_target_seconds
                .remove_label_values(&[playbook.as_str(), phase]);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum BridgeDispatchAckState {
    Acknowledged,
    Closed,
    Pending,
    Invalid,
}

impl BridgeDispatchAckState {
    fn as_str(&self) -> &'static str {
        match self {
            BridgeDispatchAckState::Acknowledged => "acknowledged",
            BridgeDispatchAckState::Closed => "closed",
            BridgeDispatchAckState::Pending => "pending",
            BridgeDispatchAckState::Invalid => "invalid",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "acknowledged" => Some(BridgeDispatchAckState::Acknowledged),
            "closed" => Some(BridgeDispatchAckState::Closed),
            "pending" => Some(BridgeDispatchAckState::Pending),
            "invalid" => Some(BridgeDispatchAckState::Invalid),
            _ => None,
        }
    }

    fn variants() -> &'static [Self] {
        &[
            BridgeDispatchAckState::Acknowledged,
            BridgeDispatchAckState::Closed,
            BridgeDispatchAckState::Pending,
            BridgeDispatchAckState::Invalid,
        ]
    }
}

#[derive(Clone, Debug)]
struct BridgeDispatchAckRecord {
    state: BridgeDispatchAckState,
    timestamp: u64,
    acknowledged: bool,
    closed: bool,
    notes: Option<String>,
}

impl BridgeDispatchAckRecord {
    fn new(
        state: BridgeDispatchAckState,
        timestamp: u64,
        acknowledged: bool,
        closed: bool,
        notes: Option<String>,
    ) -> Self {
        Self {
            state,
            timestamp,
            acknowledged,
            closed,
            notes,
        }
    }

    fn invalid(timestamp: u64, notes: String) -> Self {
        Self::new(
            BridgeDispatchAckState::Invalid,
            timestamp,
            false,
            false,
            Some(notes),
        )
    }

    fn is_completion(&self) -> bool {
        self.acknowledged || self.closed
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "state".to_string(),
            Value::String(self.state.as_str().to_string()),
        );
        map.insert("timestamp".to_string(), Value::from(self.timestamp));
        map.insert("acknowledged".to_string(), Value::Bool(self.acknowledged));
        map.insert("closed".to_string(), Value::Bool(self.closed));
        if let Some(notes) = &self.notes {
            map.insert("notes".to_string(), Value::String(notes.clone()));
        }
        Value::Object(map)
    }
}

#[derive(Clone)]
struct BridgeRemediationDispatchRecord {
    action: BridgeRemediationAction,
    target: String,
    status: String,
    dispatched_at: u64,
    acknowledgement: Option<BridgeDispatchAckRecord>,
}

impl BridgeRemediationDispatchRecord {
    fn new(
        action: BridgeRemediationAction,
        target: &str,
        status: &str,
        dispatched_at: u64,
        acknowledgement: Option<BridgeDispatchAckRecord>,
    ) -> Self {
        Self {
            action,
            target: target.to_string(),
            status: status.to_string(),
            dispatched_at,
            acknowledgement,
        }
    }

    fn to_value(&self) -> Value {
        let mut map = self.action.to_map();
        map.insert("dispatched_at".to_string(), Value::from(self.dispatched_at));
        map.insert("target".to_string(), Value::String(self.target.clone()));
        map.insert("status".to_string(), Value::String(self.status.clone()));
        if let Some(ack) = &self.acknowledgement {
            map.insert("acknowledgement".to_string(), ack.to_value());
        }
        Value::Object(map)
    }
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_text_acknowledgement(text: &str, timestamp: u64) -> Option<BridgeDispatchAckRecord> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (status_raw, trailing) = if let Some((head, tail)) = trimmed.split_once(':') {
        (head, Some(tail.trim().to_string()))
    } else if let Some((head, tail)) = trimmed.split_once(' ') {
        (head, Some(tail.trim().to_string()))
    } else {
        (trimmed, None)
    };
    let status = status_raw.trim().to_ascii_lowercase();
    let note = trailing.filter(|value| !value.is_empty());
    let record = match status.as_str() {
        "acknowledged" | "ack" | "ok" | "accepted" | "success" => BridgeDispatchAckRecord::new(
            BridgeDispatchAckState::Acknowledged,
            timestamp,
            true,
            false,
            note,
        ),
        "closed" | "resolved" | "done" | "complete" | "closed-out" => BridgeDispatchAckRecord::new(
            BridgeDispatchAckState::Closed,
            timestamp,
            true,
            true,
            note,
        ),
        "pending" | "waiting" | "open" | "queued" | "processing" | "in-progress" => {
            BridgeDispatchAckRecord::new(
                BridgeDispatchAckState::Pending,
                timestamp,
                false,
                false,
                note,
            )
        }
        "invalid" | "error" | "failed" | "rejected" | "unknown" => {
            let detail = note
                .map(|n| format!("{status}: {n}"))
                .unwrap_or_else(|| trimmed.to_string());
            BridgeDispatchAckRecord::invalid(timestamp, detail)
        }
        _ => BridgeDispatchAckRecord::invalid(timestamp, trimmed.to_string()),
    };
    Some(record)
}

fn parse_dispatch_acknowledgement(body: &[u8]) -> Option<BridgeDispatchAckRecord> {
    if body.is_empty() {
        return None;
    }
    let timestamp = unix_timestamp_secs();
    match json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => {
            let has_ack_field = map.get("acknowledged");
            let has_closed_field = map.get("closed");
            let has_any = has_ack_field.is_some() || has_closed_field.is_some();
            if !has_any {
                return None;
            }
            let closed_flag = has_closed_field.and_then(Value::as_bool).unwrap_or(false);
            let acknowledged_flag = if closed_flag {
                true
            } else {
                has_ack_field.and_then(Value::as_bool).unwrap_or(false)
            };
            let state = if closed_flag {
                BridgeDispatchAckState::Closed
            } else if acknowledged_flag {
                BridgeDispatchAckState::Acknowledged
            } else {
                BridgeDispatchAckState::Pending
            };
            let notes = map
                .get("notes")
                .and_then(Value::as_str)
                .map(|text| text.to_string());
            Some(BridgeDispatchAckRecord::new(
                state,
                timestamp,
                acknowledged_flag,
                closed_flag,
                notes,
            ))
        }
        Ok(Value::String(text)) => parse_text_acknowledgement(&text, timestamp),
        Ok(_) => Some(BridgeDispatchAckRecord::invalid(
            timestamp,
            "acknowledgement response must be a JSON object".to_string(),
        )),
        Err(_) => {
            let text = String::from_utf8_lossy(body);
            parse_text_acknowledgement(&text, timestamp)
        }
    }
}

fn update_tls_warning_snapshot(
    prefix: &str,
    code: &str,
    delta: u64,
    metadata: &TlsWarningMetadata,
    last_seen_override: Option<u64>,
) -> Option<TlsWarningUpdate> {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return None;
    };
    let now_secs = now.as_secs();
    let last_seen_hint = last_seen_override.unwrap_or(now_secs);
    let retention = TLS_WARNING_RETENTION_SECS.load(Ordering::Relaxed);
    let detail_fingerprint_value = metadata.resolved_detail_fingerprint();
    let variables_fingerprint_value = metadata.resolved_variables_fingerprint();
    let detail_bucket = fingerprint_label(detail_fingerprint_value);
    let variables_bucket = fingerprint_label(variables_fingerprint_value);
    let payload;
    let detail_new;
    let variables_new;
    let detail_unique;
    let variables_unique;
    let mut guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
    let last_seen;
    let detail_fingerprint;
    let variables_fingerprint;
    {
        let entry = guard
            .entry((prefix.to_string(), code.to_string()))
            .or_insert_with(|| TlsWarningSnapshot::new(prefix, code));
        if delta > 0 {
            entry.total = entry.total.saturating_add(delta);
            entry.last_delta = delta;
            detail_new = match entry.detail_fingerprint_counts.entry(detail_bucket.clone()) {
                Entry::Vacant(slot) => {
                    slot.insert(delta);
                    true
                }
                Entry::Occupied(mut slot) => {
                    *slot.get_mut() = slot.get().saturating_add(delta);
                    false
                }
            };
            variables_new = match entry
                .variables_fingerprint_counts
                .entry(variables_bucket.clone())
            {
                Entry::Vacant(slot) => {
                    slot.insert(delta);
                    true
                }
                Entry::Occupied(mut slot) => {
                    *slot.get_mut() = slot.get().saturating_add(delta);
                    false
                }
            };
        } else {
            detail_new = false;
            variables_new = false;
        }
        if entry.last_seen < last_seen_hint {
            entry.last_seen = last_seen_hint;
        }
        if let Some(detail) = metadata.detail.as_ref() {
            if detail.is_empty() {
                entry.detail = None;
            } else {
                entry.detail = Some(detail.clone());
            }
        }
        match detail_fingerprint_value {
            Some(fp) => entry.detail_fingerprint = Some(fp),
            None if metadata.detail.is_some() || metadata.detail_fingerprint.is_some() => {
                entry.detail_fingerprint = None;
            }
            None => {}
        }
        if !metadata.variables.is_empty() {
            entry.variables = metadata.variables.clone();
        }
        match variables_fingerprint_value {
            Some(fp) => entry.variables_fingerprint = Some(fp),
            None if !metadata.variables.is_empty() || metadata.variables_fingerprint.is_some() => {
                entry.variables_fingerprint = None;
            }
            None => {}
        }
        if metadata.origin == WarningOrigin::Diagnostics
            || entry.origin != WarningOrigin::Diagnostics
        {
            entry.origin = metadata.origin;
        }
        if let Some(peer) = metadata.peer_id.as_ref() {
            entry.peer_id = Some(peer.clone());
        }
        last_seen = entry.last_seen;
        detail_fingerprint = entry.detail_fingerprint;
        variables_fingerprint = entry.variables_fingerprint;
        detail_unique = entry.detail_fingerprint_counts.len();
        variables_unique = entry.variables_fingerprint_counts.len();
    }

    prune_tls_warning_snapshots_locked(&mut guard, now_secs);
    payload = tls_warning_status_from_guard(&guard, now_secs, retention);
    drop(guard);
    record_tls_warning_status_metrics(&payload);
    Some(TlsWarningUpdate {
        last_seen,
        detail_fingerprint,
        variables_fingerprint,
        detail_bucket,
        variables_bucket,
        detail_unique,
        variables_unique,
        detail_new,
        variables_new,
    })
}

fn tls_warning_snapshots() -> Vec<TlsWarningSnapshot> {
    TLS_WARNING_SNAPSHOTS
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect()
}

struct TlsWarningStatusPayload {
    retention_seconds: u64,
    active_snapshots: usize,
    stale_snapshots: usize,
    most_recent_last_seen: Option<u64>,
    least_recent_last_seen: Option<u64>,
}

impl TlsWarningStatusPayload {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "retention_seconds".to_string(),
            Value::from(self.retention_seconds),
        );
        map.insert(
            "active_snapshots".to_string(),
            Value::from(self.active_snapshots as u64),
        );
        map.insert(
            "stale_snapshots".to_string(),
            Value::from(self.stale_snapshots as u64),
        );
        map.insert(
            "most_recent_last_seen".to_string(),
            self.most_recent_last_seen
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "least_recent_last_seen".to_string(),
            self.least_recent_last_seen
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        Value::Object(map)
    }
}

fn tls_warning_status_snapshot() -> TlsWarningStatusPayload {
    let retention = TLS_WARNING_RETENTION_SECS.load(Ordering::Relaxed);
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
    let payload = tls_warning_status_from_guard(&guard, now_secs, retention);
    drop(guard);
    record_tls_warning_status_metrics(&payload);
    payload
}

fn tls_warning_status_from_guard(
    guard: &HashMap<(String, String), TlsWarningSnapshot>,
    now_secs: u64,
    retention: u64,
) -> TlsWarningStatusPayload {
    let mut most_recent: Option<u64> = None;
    let mut least_recent: Option<u64> = None;
    let mut stale = 0usize;
    for snapshot in guard.values() {
        let last_seen = snapshot.last_seen;
        most_recent = Some(match most_recent {
            Some(value) => value.max(last_seen),
            None => last_seen,
        });
        least_recent = Some(match least_recent {
            Some(value) => value.min(last_seen),
            None => last_seen,
        });
        if retention > 0 {
            let age = now_secs.saturating_sub(last_seen);
            if age > retention {
                stale += 1;
            }
        }
    }

    TlsWarningStatusPayload {
        retention_seconds: retention,
        active_snapshots: guard.len(),
        stale_snapshots: stale,
        most_recent_last_seen: most_recent,
        least_recent_last_seen: least_recent,
    }
}

fn record_tls_warning_status_metrics(payload: &TlsWarningStatusPayload) {
    gauge!(
        METRIC_TLS_ENV_WARNING_RETENTION_SECONDS,
        payload.retention_seconds as f64
    );
    gauge!(
        METRIC_TLS_ENV_WARNING_ACTIVE_SNAPSHOTS,
        payload.active_snapshots as f64
    );
    gauge!(
        METRIC_TLS_ENV_WARNING_STALE_SNAPSHOTS,
        payload.stale_snapshots as f64
    );
    gauge!(
        METRIC_TLS_ENV_WARNING_MOST_RECENT_LAST_SEEN,
        payload.most_recent_last_seen.unwrap_or(0) as f64
    );
    gauge!(
        METRIC_TLS_ENV_WARNING_LEAST_RECENT_LAST_SEEN,
        payload.least_recent_last_seen.unwrap_or(0) as f64
    );
}

fn prune_tls_warning_snapshots_locked(
    snapshots: &mut HashMap<(String, String), TlsWarningSnapshot>,
    now_secs: u64,
) {
    let retention = TLS_WARNING_RETENTION_SECS.load(Ordering::Relaxed);
    if retention == 0 {
        return;
    }
    let cutoff = now_secs.saturating_sub(retention);
    snapshots.retain(|_, snapshot| snapshot.last_seen >= cutoff);
}

#[cfg(test)]
fn reset_tls_warning_snapshots() {
    TLS_WARNING_SNAPSHOTS.lock().unwrap().clear();
    TLS_WARNING_RETENTION_SECS.store(TLS_WARNING_SNAPSHOT_RETENTION_SECS, Ordering::Relaxed);
    reset_tls_warning_status_metrics();
}

#[cfg(test)]
fn tls_warning_snapshot(prefix: &str, code: &str) -> Option<TlsWarningSnapshot> {
    tls_warning_snapshots()
        .into_iter()
        .find(|entry| entry.prefix == prefix && entry.code == code)
}

#[cfg(test)]
fn prune_tls_warning_snapshots_for_test(now_secs: u64) {
    let mut guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
    prune_tls_warning_snapshots_locked(&mut guard, now_secs);
    let payload = tls_warning_status_from_guard(
        &guard,
        now_secs,
        TLS_WARNING_RETENTION_SECS.load(Ordering::Relaxed),
    );
    drop(guard);
    record_tls_warning_status_metrics(&payload);
}

#[cfg(test)]
fn reset_tls_warning_status_metrics() {
    let metrics = aggregator_metrics();
    metrics.tls_env_warning_total.reset();
    metrics.tls_env_warning_events_total.reset();
    metrics.tls_env_warning_retention_seconds.reset();
    metrics.tls_env_warning_active_snapshots.reset();
    metrics.tls_env_warning_stale_snapshots.reset();
    metrics.tls_env_warning_most_recent_last_seen.reset();
    metrics.tls_env_warning_least_recent_last_seen.reset();
    metrics.tls_env_warning_detail_fingerprint.reset();
    metrics.tls_env_warning_variables_fingerprint.reset();
    metrics.tls_env_warning_detail_fingerprint_total.reset();
    metrics.tls_env_warning_variables_fingerprint_total.reset();
    metrics.tls_env_warning_detail_unique_fingerprints.reset();
    metrics
        .tls_env_warning_variables_unique_fingerprints
        .reset();
}

static TLS_WARNING_SINK: OnceLock<TlsEnvWarningSinkGuard> = OnceLock::new();
static TLS_WARNING_SUBSCRIBER: OnceLock<LoggingSubscriberGuard> = OnceLock::new();

fn record_tls_env_warning_event(
    prefix: &str,
    code: &str,
    delta: u64,
    metadata: TlsWarningMetadata,
) {
    if delta == 0 {
        return;
    }
    increment_counter!(
        METRIC_TLS_ENV_WARNING_TOTAL,
        delta,
        "prefix" => prefix,
        "code" => code
    );
    increment_counter!(
        METRIC_TLS_ENV_WARNING_EVENTS_TOTAL,
        delta,
        "prefix" => prefix,
        "code" => code,
        "origin" => metadata.origin.as_str()
    );
    let last_seen = update_tls_warning_snapshot(prefix, code, delta, &metadata, None);
    if let Some(update) = last_seen {
        gauge!(
            METRIC_TLS_ENV_WARNING_LAST_SEEN,
            update.last_seen as f64,
            "prefix" => prefix,
            "code" => code
        );
        record_tls_warning_fingerprint_metrics(prefix, code, &update);
        record_tls_warning_fingerprint_counters(prefix, code, delta, &update);
    }
}

fn record_tls_env_warning_last_seen(
    prefix: &str,
    code: &str,
    last_seen_secs: u64,
    metadata: TlsWarningMetadata,
) {
    if last_seen_secs == 0 {
        return;
    }
    let last_seen = update_tls_warning_snapshot(prefix, code, 0, &metadata, Some(last_seen_secs));
    if let Some(update) = last_seen {
        gauge!(
            METRIC_TLS_ENV_WARNING_LAST_SEEN,
            update.last_seen as f64,
            "prefix" => prefix,
            "code" => code
        );
        record_tls_warning_fingerprint_metrics(prefix, code, &update);
    }
}

fn record_tls_warning_fingerprint_metrics(prefix: &str, code: &str, update: &TlsWarningUpdate) {
    let metrics = aggregator_metrics();
    if let Err(err) = metrics
        .tls_env_warning_detail_fingerprint
        .ensure_handle_for_label_values(&[prefix, code])
        .map(|handle| handle.set(update.detail_fingerprint.unwrap_or(0)))
    {
        warn!(
            target: "aggregator",
            %prefix,
            %code,
            error = %err,
            "failed to record tls env warning detail fingerprint",
        );
    }
    if let Err(err) = metrics
        .tls_env_warning_variables_fingerprint
        .ensure_handle_for_label_values(&[prefix, code])
        .map(|handle| handle.set(update.variables_fingerprint.unwrap_or(0)))
    {
        warn!(
            target: "aggregator",
            %prefix,
            %code,
            error = %err,
            "failed to record tls env warning variables fingerprint",
        );
    }
    gauge!(
        METRIC_TLS_ENV_WARNING_DETAIL_UNIQUE_FINGERPRINTS,
        update.detail_unique as f64,
        "prefix" => prefix,
        "code" => code
    );
    gauge!(
        METRIC_TLS_ENV_WARNING_VARIABLES_UNIQUE_FINGERPRINTS,
        update.variables_unique as f64,
        "prefix" => prefix,
        "code" => code
    );
}

fn record_tls_warning_fingerprint_counters(
    prefix: &str,
    code: &str,
    delta: u64,
    update: &TlsWarningUpdate,
) {
    if delta == 0 {
        return;
    }
    increment_counter!(
        METRIC_TLS_ENV_WARNING_DETAIL_FINGERPRINT_TOTAL,
        delta,
        "prefix" => prefix,
        "code" => code,
        "fingerprint" => update.detail_bucket.as_str()
    );
    if delta > 0 && update.detail_new && update.detail_bucket != "none" {
        info!(
            target: "aggregator",
            prefix = %prefix,
            code = %code,
            fingerprint = %update.detail_bucket,
            "observed new tls env warning detail fingerprint",
        );
    }
    increment_counter!(
        METRIC_TLS_ENV_WARNING_VARIABLES_FINGERPRINT_TOTAL,
        delta,
        "prefix" => prefix,
        "code" => code,
        "fingerprint" => update.variables_bucket.as_str()
    );
    if delta > 0 && update.variables_new && update.variables_bucket != "none" {
        info!(
            target: "aggregator",
            prefix = %prefix,
            code = %code,
            fingerprint = %update.variables_bucket,
            "observed new tls env warning variables fingerprint",
        );
    }
}

fn ensure_tls_warning_forwarder() {
    ensure_foundation_metrics_recorder();
    TLS_WARNING_SINK.get_or_init(|| {
        register_tls_warning_sink(|warning| {
            record_tls_env_warning_event(
                &warning.prefix,
                warning.code,
                1,
                TlsWarningMetadata::diagnostics(warning.detail.clone(), warning.variables.clone()),
            );
        })
    });
    TLS_WARNING_SUBSCRIBER.get_or_init(|| {
        install_tls_env_warning_subscriber(|warning| {
            if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                record_tls_env_warning_last_seen(
                    &warning.prefix,
                    &warning.code,
                    now.as_secs(),
                    TlsWarningMetadata::diagnostics(
                        warning.detail.clone(),
                        warning.variables.clone(),
                    ),
                );
            }
        })
    });
}

pub fn install_tls_env_warning_forwarder() {
    ensure_tls_warning_forwarder();
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BridgeAnomalyLabel {
    key: String,
    value: String,
}

#[derive(Clone, Debug, PartialEq)]
struct BridgeAnomalyEvent {
    metric: String,
    peer_id: String,
    labels: Vec<BridgeAnomalyLabel>,
    delta: f64,
    mean: f64,
    stddev: f64,
    threshold: f64,
    window: usize,
    timestamp: u64,
}

impl BridgeAnomalyLabel {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("key".to_string(), Value::String(self.key.clone()));
        map.insert("value".to_string(), Value::String(self.value.clone()));
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let key = object.get("key").and_then(Value::as_str)?;
        let val = object.get("value").and_then(Value::as_str)?;
        Some(Self {
            key: key.to_string(),
            value: val.to_string(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd)]
enum BridgeRemediationActionType {
    Page,
    Throttle,
    Quarantine,
    Escalate,
}

impl BridgeRemediationActionType {
    fn as_str(&self) -> &'static str {
        match self {
            BridgeRemediationActionType::Page => "page",
            BridgeRemediationActionType::Throttle => "throttle",
            BridgeRemediationActionType::Quarantine => "quarantine",
            BridgeRemediationActionType::Escalate => "escalate",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "page" => Some(BridgeRemediationActionType::Page),
            "throttle" => Some(BridgeRemediationActionType::Throttle),
            "quarantine" => Some(BridgeRemediationActionType::Quarantine),
            "escalate" => Some(BridgeRemediationActionType::Escalate),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum BridgeRemediationPlaybook {
    None,
    IncentiveThrottle,
    GovernanceEscalation,
}

impl BridgeRemediationPlaybook {
    fn as_str(&self) -> &'static str {
        match self {
            BridgeRemediationPlaybook::None => "none",
            BridgeRemediationPlaybook::IncentiveThrottle => "incentive-throttle",
            BridgeRemediationPlaybook::GovernanceEscalation => "governance-escalation",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            BridgeRemediationPlaybook::None => "operator paging",
            BridgeRemediationPlaybook::IncentiveThrottle => "incentive throttle",
            BridgeRemediationPlaybook::GovernanceEscalation => "governance escalation",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(BridgeRemediationPlaybook::None),
            "incentive-throttle" => Some(BridgeRemediationPlaybook::IncentiveThrottle),
            "governance-escalation" => Some(BridgeRemediationPlaybook::GovernanceEscalation),
            _ => None,
        }
    }

    fn variants() -> &'static [Self] {
        const VARIANTS: [BridgeRemediationPlaybook; 3] = [
            BridgeRemediationPlaybook::None,
            BridgeRemediationPlaybook::IncentiveThrottle,
            BridgeRemediationPlaybook::GovernanceEscalation,
        ];
        &VARIANTS
    }
}

impl BridgeAnomalyEvent {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("metric".to_string(), Value::String(self.metric.clone()));
        map.insert("peer_id".to_string(), Value::String(self.peer_id.clone()));
        let labels = self
            .labels
            .iter()
            .map(BridgeAnomalyLabel::to_value)
            .collect();
        map.insert("labels".to_string(), Value::Array(labels));
        map.insert("delta".to_string(), Value::from(self.delta));
        map.insert("mean".to_string(), Value::from(self.mean));
        map.insert("stddev".to_string(), Value::from(self.stddev));
        map.insert("threshold".to_string(), Value::from(self.threshold));
        map.insert("window".to_string(), Value::from(self.window as u64));
        map.insert("timestamp".to_string(), Value::from(self.timestamp));
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let metric = object.get("metric").and_then(Value::as_str)?.to_string();
        let peer_id = object.get("peer_id").and_then(Value::as_str)?.to_string();
        let labels = object
            .get("labels")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(BridgeAnomalyLabel::from_value)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let delta = object.get("delta").and_then(Value::as_f64)?;
        let mean = object.get("mean").and_then(Value::as_f64)?;
        let stddev = object.get("stddev").and_then(Value::as_f64)?;
        let threshold = object.get("threshold").and_then(Value::as_f64)?;
        let window = object.get("window").and_then(Value::as_u64)? as usize;
        let timestamp = object.get("timestamp").and_then(Value::as_u64)?;
        Some(Self {
            metric,
            peer_id,
            labels,
            delta,
            mean,
            stddev,
            threshold,
            window,
            timestamp,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct BridgeRemediationAction {
    peer_id: String,
    metric: String,
    labels: Vec<BridgeAnomalyLabel>,
    action: BridgeRemediationActionType,
    playbook: BridgeRemediationPlaybook,
    occurrences: usize,
    delta: f64,
    threshold: f64,
    ratio: f64,
    timestamp: u64,
    acknowledged_at: Option<u64>,
    closed_out_at: Option<u64>,
    acknowledgement_notes: Option<String>,
    first_dispatch_at: Option<u64>,
    last_dispatch_at: Option<u64>,
    dispatch_attempts: u32,
    auto_retry_count: u32,
    last_auto_retry_at: Option<u64>,
    pending_since: Option<u64>,
    pending_escalated: bool,
    last_ack_state: Option<BridgeDispatchAckState>,
    last_ack_notes: Option<String>,
    follow_up_notes: Option<String>,
}

#[derive(Clone)]
enum BridgeRemediationFollowUp {
    Retry { action: BridgeRemediationAction },
    Escalate { escalation: BridgeRemediationAction },
}

#[derive(Clone, Copy)]
enum BridgeRemediationDispatchOrigin {
    Anomaly,
    AutoRetry,
    AutoEscalation,
}

impl BridgeRemediationAction {
    fn new(
        event: &BridgeAnomalyEvent,
        action: BridgeRemediationActionType,
        occurrences: usize,
        ratio: f64,
    ) -> Self {
        let playbook = match action {
            BridgeRemediationActionType::Page => BridgeRemediationPlaybook::None,
            BridgeRemediationActionType::Throttle | BridgeRemediationActionType::Quarantine => {
                BridgeRemediationPlaybook::IncentiveThrottle
            }
            BridgeRemediationActionType::Escalate => {
                BridgeRemediationPlaybook::GovernanceEscalation
            }
        };
        Self {
            peer_id: event.peer_id.clone(),
            metric: event.metric.clone(),
            labels: event.labels.clone(),
            action,
            playbook,
            occurrences,
            delta: event.delta,
            threshold: event.threshold,
            ratio,
            timestamp: event.timestamp,
            acknowledged_at: None,
            closed_out_at: None,
            acknowledgement_notes: None,
            first_dispatch_at: None,
            last_dispatch_at: None,
            dispatch_attempts: 0,
            auto_retry_count: 0,
            last_auto_retry_at: None,
            pending_since: None,
            pending_escalated: false,
            last_ack_state: None,
            last_ack_notes: None,
            follow_up_notes: None,
        }
    }

    fn labels_summary(&self) -> Vec<String> {
        self.labels
            .iter()
            .map(|label| format!("{}={}", label.key, label.value))
            .collect()
    }

    fn ratio_phrase(&self) -> String {
        if self.ratio.is_finite() && self.ratio > 0.0 {
            format!("{:.2}× baseline", self.ratio)
        } else {
            "baseline threshold crossed".to_string()
        }
    }

    fn annotation(&self) -> String {
        let labels = self.labels_summary();
        let label_clause = if labels.is_empty() {
            "no label qualifiers".to_string()
        } else {
            format!("labels [{}]", labels.join(", "))
        };
        format!(
            "Peer {} triggered {} for {} ({}, delta {:.2}) after {} samples – executing the {} playbook with {}.",
            self.peer_id,
            self.action.as_str(),
            self.metric,
            self.ratio_phrase(),
            self.delta,
            self.occurrences,
            self.playbook.display_name(),
            label_clause
        )
    }

    fn dashboard_panels(&self) -> Vec<String> {
        let mut panels: Vec<String> = BRIDGE_REMEDIATION_BASE_PANELS
            .iter()
            .chain(BRIDGE_LIQUIDITY_PANELS.iter())
            .map(|panel| (*panel).to_string())
            .collect();
        let mut extras: Vec<String> = match self.metric.as_str() {
            "bridge_reward_claims_total" => vec![
                BRIDGE_PANEL_REWARD_CLAIMS.to_string(),
                BRIDGE_PANEL_REWARD_APPROVALS.to_string(),
            ],
            "bridge_reward_approvals_consumed_total" => vec![
                BRIDGE_PANEL_REWARD_APPROVALS.to_string(),
                BRIDGE_PANEL_REWARD_CLAIMS.to_string(),
            ],
            "bridge_settlement_results_total" => {
                vec![BRIDGE_PANEL_SETTLEMENT_RESULTS.to_string()]
            }
            "bridge_dispute_outcomes_total" => {
                vec![BRIDGE_PANEL_DISPUTE_OUTCOMES.to_string()]
            }
            metric => vec![format!("{} (5m delta)", metric)],
        };
        panels.append(&mut extras);
        panels.sort();
        panels.dedup();
        panels
    }

    fn response_sequence_with_panels(&self, panels: &[String]) -> Vec<String> {
        let panel_clause = if panels.is_empty() {
            "bridge remediation dashboard row".to_string()
        } else {
            panels.join(", ")
        };
        let dispatch_step = format!(
            "Audit remediation persistence at /remediation/bridge and dispatch status via {}.",
            BRIDGE_REMEDIATION_DISPATCH_ENDPOINT
        );
        match self.playbook {
            BridgeRemediationPlaybook::None => vec![
                format!(
                    "Acknowledge the bridge remediation page for peer {} on metric {} (action {}).",
                    self.peer_id,
                    self.metric,
                    self.action.as_str()
                ),
                format!("Review Grafana panels: {}.", panel_clause),
                dispatch_step.clone(),
                "Coordinate with the relayer and confirm the metric returns to baseline before closing the alert."
                    .to_string(),
            ],
            BridgeRemediationPlaybook::IncentiveThrottle => vec![
                format!(
                    "Activate the incentive throttle runbook for peer {} on metric {} (action {}).",
                    self.peer_id,
                    self.metric,
                    self.action.as_str()
                ),
                format!("Review Grafana panels: {}.", panel_clause),
                format!(
                    "Execute throttle or quarantine steps documented in {}.",
                    BRIDGE_REMEDIATION_RUNBOOK_PATH
                ),
                dispatch_step.clone(),
                "Schedule a follow-up to unwind throttles once liquidity and approvals stabilize."
                    .to_string(),
            ],
            BridgeRemediationPlaybook::GovernanceEscalation => vec![
                format!(
                    "Escalate the bridge remediation to governance for peer {} on metric {}.",
                    self.peer_id, self.metric
                ),
                format!("Review Grafana panels: {}.", panel_clause),
                format!(
                    "Open or update the governance incident as outlined in {} and copy the annotation into the record.",
                    BRIDGE_REMEDIATION_RUNBOOK_PATH
                ),
                dispatch_step,
                "Coordinate cross-chain liquidity fallback and monitor until metrics return to baseline before closing the governance item.".to_string(),
            ],
        }
    }

    fn to_map(&self) -> Map {
        let mut map = Map::new();
        map.insert("peer_id".to_string(), Value::String(self.peer_id.clone()));
        map.insert("metric".to_string(), Value::String(self.metric.clone()));
        map.insert(
            "labels".to_string(),
            Value::Array(
                self.labels
                    .iter()
                    .map(BridgeAnomalyLabel::to_value)
                    .collect(),
            ),
        );
        map.insert(
            "action".to_string(),
            Value::String(self.action.as_str().to_string()),
        );
        map.insert(
            "playbook".to_string(),
            Value::String(self.playbook.as_str().to_string()),
        );
        map.insert(
            "occurrences".to_string(),
            Value::from(self.occurrences as u64),
        );
        map.insert("delta".to_string(), Value::from(self.delta));
        map.insert("threshold".to_string(), Value::from(self.threshold));
        map.insert("ratio".to_string(), Value::from(self.ratio));
        map.insert("timestamp".to_string(), Value::from(self.timestamp));
        map.insert("annotation".to_string(), Value::String(self.annotation()));
        map.insert(
            "acknowledged_at".to_string(),
            self.acknowledged_at.map(Value::from).unwrap_or(Value::Null),
        );
        map.insert(
            "closed_out_at".to_string(),
            self.closed_out_at.map(Value::from).unwrap_or(Value::Null),
        );
        map.insert(
            "acknowledgement_notes".to_string(),
            self.acknowledgement_notes
                .as_ref()
                .map(|notes| Value::String(notes.clone()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "first_dispatch_at".to_string(),
            self.first_dispatch_at
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "last_dispatch_at".to_string(),
            self.last_dispatch_at
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "dispatch_attempts".to_string(),
            Value::from(self.dispatch_attempts as u64),
        );
        map.insert(
            "auto_retry_count".to_string(),
            Value::from(self.auto_retry_count as u64),
        );
        map.insert(
            "last_auto_retry_at".to_string(),
            self.last_auto_retry_at
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "pending_since".to_string(),
            self.pending_since.map(Value::from).unwrap_or(Value::Null),
        );
        map.insert(
            "pending_escalated".to_string(),
            Value::Bool(self.pending_escalated),
        );
        map.insert(
            "last_ack_state".to_string(),
            self.last_ack_state
                .map(|state| Value::String(state.as_str().to_string()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "last_ack_notes".to_string(),
            self.last_ack_notes
                .as_ref()
                .map(|notes| Value::String(notes.clone()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "follow_up_notes".to_string(),
            self.follow_up_notes
                .as_ref()
                .map(|notes| Value::String(notes.clone()))
                .unwrap_or(Value::Null),
        );
        let panels = self.dashboard_panels();
        map.insert(
            "dashboard_panels".to_string(),
            Value::Array(panels.iter().cloned().map(Value::String).collect()),
        );
        map.insert(
            "runbook_path".to_string(),
            Value::String(BRIDGE_REMEDIATION_RUNBOOK_PATH.to_string()),
        );
        map.insert(
            "dispatch_endpoint".to_string(),
            Value::String(BRIDGE_REMEDIATION_DISPATCH_ENDPOINT.to_string()),
        );
        map.insert(
            "response_sequence".to_string(),
            Value::Array(
                self.response_sequence_with_panels(&panels)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
        map
    }

    fn to_value(&self) -> Value {
        Value::Object(self.to_map())
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let peer_id = object.get("peer_id").and_then(Value::as_str)?.to_string();
        let metric = object.get("metric").and_then(Value::as_str)?.to_string();
        let labels = object
            .get("labels")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(BridgeAnomalyLabel::from_value)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .and_then(BridgeRemediationActionType::from_str)?;
        let playbook = object
            .get("playbook")
            .and_then(Value::as_str)
            .and_then(BridgeRemediationPlaybook::from_str)
            .unwrap_or(match action {
                BridgeRemediationActionType::Page => BridgeRemediationPlaybook::None,
                BridgeRemediationActionType::Throttle | BridgeRemediationActionType::Quarantine => {
                    BridgeRemediationPlaybook::IncentiveThrottle
                }
                BridgeRemediationActionType::Escalate => {
                    BridgeRemediationPlaybook::GovernanceEscalation
                }
            });
        let occurrences = object
            .get("occurrences")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let delta = object.get("delta").and_then(Value::as_f64)?;
        let threshold = object.get("threshold").and_then(Value::as_f64)?;
        let ratio = object.get("ratio").and_then(Value::as_f64).unwrap_or(0.0);
        let timestamp = object.get("timestamp").and_then(Value::as_u64)?;
        let acknowledged_at = match object.get("acknowledged_at") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let closed_out_at = match object.get("closed_out_at") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let acknowledgement_notes = object
            .get("acknowledgement_notes")
            .and_then(Value::as_str)
            .map(|text| text.to_string());
        let first_dispatch_at = match object.get("first_dispatch_at") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let last_dispatch_at = match object.get("last_dispatch_at") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let dispatch_attempts = object
            .get("dispatch_attempts")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let auto_retry_count = object
            .get("auto_retry_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let last_auto_retry_at = match object.get("last_auto_retry_at") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let pending_since = match object.get("pending_since") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let pending_escalated = object
            .get("pending_escalated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let last_ack_state = object
            .get("last_ack_state")
            .and_then(Value::as_str)
            .and_then(BridgeDispatchAckState::from_str);
        let last_ack_notes = object
            .get("last_ack_notes")
            .and_then(Value::as_str)
            .map(|text| text.to_string());
        let follow_up_notes = object
            .get("follow_up_notes")
            .and_then(Value::as_str)
            .map(|text| text.to_string());
        Some(Self {
            peer_id,
            metric,
            labels,
            action,
            playbook,
            occurrences,
            delta,
            threshold,
            ratio,
            timestamp,
            acknowledged_at,
            closed_out_at,
            acknowledgement_notes,
            first_dispatch_at,
            last_dispatch_at,
            dispatch_attempts,
            auto_retry_count,
            last_auto_retry_at,
            pending_since,
            pending_escalated,
            last_ack_state,
            last_ack_notes,
            follow_up_notes,
        })
    }
}

#[derive(Clone, Default)]
struct BridgeRemediationHooks {
    page: Vec<BridgeRemediationHook>,
    throttle: Vec<BridgeRemediationHook>,
    quarantine: Vec<BridgeRemediationHook>,
    escalate: Vec<BridgeRemediationHook>,
}

impl BridgeRemediationHooks {
    fn from_env() -> Self {
        Self {
            page: collect_hooks(ENV_REMEDIATION_PAGE_URLS, ENV_REMEDIATION_PAGE_DIRS),
            throttle: collect_hooks(ENV_REMEDIATION_THROTTLE_URLS, ENV_REMEDIATION_THROTTLE_DIRS),
            quarantine: collect_hooks(
                ENV_REMEDIATION_QUARANTINE_URLS,
                ENV_REMEDIATION_QUARANTINE_DIRS,
            ),
            escalate: collect_hooks(ENV_REMEDIATION_ESCALATE_URLS, ENV_REMEDIATION_ESCALATE_DIRS),
        }
    }

    fn dispatch(&self, state: AppState, action: &BridgeRemediationAction) {
        let targets = match action.action {
            BridgeRemediationActionType::Page => &self.page,
            BridgeRemediationActionType::Throttle => &self.throttle,
            BridgeRemediationActionType::Quarantine => &self.quarantine,
            BridgeRemediationActionType::Escalate => &self.escalate,
        };
        if targets.is_empty() {
            record_dispatch_outcome(None, action, "none", "skipped", None);
            return;
        }
        for target in targets {
            target.dispatch(state.clone(), action);
        }
    }
}

#[derive(Clone)]
enum BridgeRemediationHook {
    Http { url: String },
    File { dir: PathBuf },
}

impl BridgeRemediationHook {
    fn dispatch(&self, state: AppState, action: &BridgeRemediationAction) {
        match self {
            BridgeRemediationHook::Http { url } => {
                let url = url.clone();
                let payload = build_dispatch_payload(action);
                let summary = action.clone();
                let state = state.clone();
                if let Some(client) = bridge_http_client_override() {
                    let payload = payload.clone();
                    spawn(async move {
                        match client.send(&url, &payload) {
                            Ok(response) => {
                                let status = response.status;
                                if status.is_success() {
                                    let ack = parse_dispatch_acknowledgement(&response.body);
                                    if let Some(ack_record) = ack.as_ref() {
                                        if matches!(
                                            ack_record.state,
                                            BridgeDispatchAckState::Invalid
                                        ) {
                                            warn!(
                                                target: "aggregator",
                                                url = %url,
                                                peer = %summary.peer_id,
                                                metric = %summary.metric,
                                                action = summary.action.as_str(),
                                                playbook = summary.playbook.as_str(),
                                                notes = ack_record
                                                    .notes
                                                    .as_deref()
                                                    .unwrap_or(""),
                                                "bridge remediation http acknowledgement parse failed",
                                            );
                                        }
                                    }
                                    info!(
                                        target: "aggregator",
                                        url = %url,
                                        status = status.as_u16(),
                                        peer = %summary.peer_id,
                                        metric = %summary.metric,
                                        action = summary.action.as_str(),
                                        playbook = summary.playbook.as_str(),
                                        "bridge remediation hook dispatched via http",
                                    );
                                    record_dispatch_outcome(
                                        Some(state.clone()),
                                        &summary,
                                        "http",
                                        "success",
                                        ack,
                                    );
                                } else {
                                    warn!(
                                        target: "aggregator",
                                        url = %url,
                                        status = status.as_u16(),
                                        peer = %summary.peer_id,
                                        metric = %summary.metric,
                                        action = summary.action.as_str(),
                                        playbook = summary.playbook.as_str(),
                                        "bridge remediation http dispatch returned non-success status",
                                    );
                                    record_dispatch_outcome(
                                        Some(state.clone()),
                                        &summary,
                                        "http",
                                        "status_failed",
                                        None,
                                    );
                                }
                            }
                            Err(err) => {
                                warn!(
                                    target: "aggregator",
                                    error = %err,
                                    url = %url,
                                    peer = %summary.peer_id,
                                    metric = %summary.metric,
                                    action = summary.action.as_str(),
                                    playbook = summary.playbook.as_str(),
                                    "bridge remediation http override dispatch failed",
                                );
                                record_dispatch_outcome(
                                    Some(state.clone()),
                                    &summary,
                                    "http",
                                    "request_failed",
                                    None,
                                );
                            }
                        }
                    });
                } else {
                    spawn(async move {
                        let client = http_client();
                        let request = match client.request(Method::Post, &url) {
                            Ok(builder) => builder,
                            Err(err) => {
                                warn!(
                                    target: "aggregator",
                                    error = %err,
                                    url = %url,
                                    peer = %summary.peer_id,
                                    metric = %summary.metric,
                                    action = summary.action.as_str(),
                                    playbook = summary.playbook.as_str(),
                                    "bridge remediation http dispatch failed to build request",
                                );
                                record_dispatch_outcome(
                                    Some(state.clone()),
                                    &summary,
                                    "http",
                                    "request_build_failed",
                                    None,
                                );
                                return;
                            }
                        };
                        let request = match request.json(&payload) {
                            Ok(req) => req,
                            Err(err) => {
                                warn!(
                                    target: "aggregator",
                                    error = %err,
                                    url = %url,
                                    peer = %summary.peer_id,
                                    metric = %summary.metric,
                                    action = summary.action.as_str(),
                                    playbook = summary.playbook.as_str(),
                                    "bridge remediation http dispatch failed to encode payload",
                                );
                                record_dispatch_outcome(
                                    Some(state.clone()),
                                    &summary,
                                    "http",
                                    "payload_encode_failed",
                                    None,
                                );
                                return;
                            }
                        };
                        match request.send().await {
                            Ok(response) => {
                                let status = response.status();
                                if status.is_success() {
                                    let ack = parse_dispatch_acknowledgement(response.body());
                                    if let Some(ack_record) = ack.as_ref() {
                                        if matches!(
                                            ack_record.state,
                                            BridgeDispatchAckState::Invalid
                                        ) {
                                            warn!(
                                                target: "aggregator",
                                                url = %url,
                                                peer = %summary.peer_id,
                                                metric = %summary.metric,
                                                action = summary.action.as_str(),
                                                playbook = summary.playbook.as_str(),
                                                notes = ack_record
                                                    .notes
                                                    .as_deref()
                                                    .unwrap_or(""),
                                                "bridge remediation http acknowledgement parse failed",
                                            );
                                        }
                                    }
                                    info!(
                                        target: "aggregator",
                                        url = %url,
                                        status = status.as_u16(),
                                        peer = %summary.peer_id,
                                        metric = %summary.metric,
                                        action = summary.action.as_str(),
                                        playbook = summary.playbook.as_str(),
                                        "bridge remediation hook dispatched via http",
                                    );
                                    record_dispatch_outcome(
                                        Some(state.clone()),
                                        &summary,
                                        "http",
                                        "success",
                                        ack,
                                    );
                                } else {
                                    warn!(
                                        target: "aggregator",
                                        url = %url,
                                        status = status.as_u16(),
                                        peer = %summary.peer_id,
                                        metric = %summary.metric,
                                        action = summary.action.as_str(),
                                        playbook = summary.playbook.as_str(),
                                        "bridge remediation http dispatch returned non-success status",
                                    );
                                    record_dispatch_outcome(
                                        Some(state.clone()),
                                        &summary,
                                        "http",
                                        "status_failed",
                                        None,
                                    );
                                }
                            }
                            Err(err) => {
                                warn!(
                                    target: "aggregator",
                                    error = %err,
                                    url = %url,
                                    peer = %summary.peer_id,
                                    metric = %summary.metric,
                                    action = summary.action.as_str(),
                                    playbook = summary.playbook.as_str(),
                                    "bridge remediation http dispatch failed",
                                );
                                record_dispatch_outcome(
                                    Some(state.clone()),
                                    &summary,
                                    "http",
                                    "request_failed",
                                    None,
                                );
                            }
                        }
                    });
                }
            }
            BridgeRemediationHook::File { dir } => {
                let dir = dir.clone();
                let summary = action.clone();
                let state = state.clone();
                spawn(async move {
                    let payload = build_dispatch_payload(&summary);
                    let handle = spawn_blocking(move || persist_action_to_dir(dir, payload));
                    match handle.await {
                        Ok(Ok(path)) => {
                            info!(
                                target: "aggregator",
                                path = %path.display(),
                                peer = %summary.peer_id,
                                metric = %summary.metric,
                                action = summary.action.as_str(),
                                playbook = summary.playbook.as_str(),
                                "bridge remediation hook persisted to spool",
                            );
                            record_dispatch_outcome(
                                Some(state.clone()),
                                &summary,
                                "spool",
                                "success",
                                None,
                            );
                        }
                        Ok(Err(err)) => {
                            warn!(
                                target: "aggregator",
                                error = %err,
                                peer = %summary.peer_id,
                                metric = %summary.metric,
                                action = summary.action.as_str(),
                                playbook = summary.playbook.as_str(),
                                "bridge remediation spool dispatch failed",
                            );
                            record_dispatch_outcome(
                                Some(state.clone()),
                                &summary,
                                "spool",
                                "persist_failed",
                                None,
                            );
                        }
                        Err(err) => {
                            warn!(
                                target: "aggregator",
                                error = %err,
                                peer = %summary.peer_id,
                                metric = %summary.metric,
                                action = summary.action.as_str(),
                                playbook = summary.playbook.as_str(),
                                "bridge remediation spool dispatch join failed",
                            );
                            record_dispatch_outcome(
                                Some(state.clone()),
                                &summary,
                                "spool",
                                "join_failed",
                                None,
                            );
                        }
                    }
                });
            }
        }
    }
}

fn record_dispatch_outcome(
    state: Option<AppState>,
    action: &BridgeRemediationAction,
    target: &str,
    status: &str,
    acknowledgement: Option<BridgeDispatchAckRecord>,
) {
    let metrics = aggregator_metrics();
    metrics
        .bridge_remediation_dispatch_total
        .with_label_values(&[
            action.action.as_str(),
            action.playbook.as_str(),
            target,
            status,
        ])
        .inc();
    if let Some(ack) = &acknowledgement {
        metrics
            .bridge_remediation_dispatch_ack_total
            .with_label_values(&[
                action.action.as_str(),
                action.playbook.as_str(),
                target,
                ack.state.as_str(),
            ])
            .inc();
    }
    let dispatched_at = unix_timestamp_secs();
    let dispatch_update = state.as_ref().and_then(|state| {
        state.record_bridge_dispatch(
            action,
            acknowledgement.as_ref(),
            dispatched_at,
            target,
            status,
        )
    });
    if let Some(update) = dispatch_update.as_ref() {
        if let Some(sample) = update.ack_sample.as_ref() {
            let handle = metrics
                .bridge_remediation_ack_latency_seconds
                .with_label_values(&[sample.playbook.as_str(), sample.state.as_str()]);
            for _ in 0..sample.count {
                handle.observe(sample.latency as f64);
            }
        }
    }
    let record_action = dispatch_update
        .as_ref()
        .map(|update| update.action.clone())
        .unwrap_or_else(|| action.clone());
    let record = BridgeRemediationDispatchRecord::new(
        record_action,
        target,
        status,
        dispatched_at,
        acknowledgement,
    );
    if let Ok(mut guard) = bridge_dispatch_log().lock() {
        guard.push_back(record);
        if guard.len() > BRIDGE_REMEDIATION_MAX_DISPATCH_LOG {
            guard.pop_front();
        }
    }
}

fn collect_hooks(url_key: &str, dir_key: &str) -> Vec<BridgeRemediationHook> {
    let mut hooks = Vec::new();
    for url in parse_env_list(url_key) {
        hooks.push(BridgeRemediationHook::Http { url });
    }
    for dir in parse_env_list(dir_key) {
        hooks.push(BridgeRemediationHook::File {
            dir: PathBuf::from(dir),
        });
    }
    hooks
}

fn parse_env_list(key: &str) -> Vec<String> {
    match env::var(key) {
        Ok(value) => value
            .split(|c: char| matches!(c, ',' | ';' | '\n' | '\r'))
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn build_dispatch_payload(action: &BridgeRemediationAction) -> Value {
    let mut payload = match action.to_value() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let dispatched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    payload.insert("dispatched_at".to_string(), Value::from(dispatched_at));
    Value::Object(payload)
}

fn persist_action_to_dir(dir: PathBuf, payload: Value) -> io::Result<PathBuf> {
    fs::create_dir_all(&dir)?;
    let sequence = BRIDGE_REMEDIATION_DISPATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let action = payload
        .as_object()
        .and_then(|map| map.get("action"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let peer = payload
        .as_object()
        .and_then(|map| map.get("peer_id"))
        .and_then(Value::as_str)
        .unwrap_or("peer");
    let metric = payload
        .as_object()
        .and_then(|map| map.get("metric"))
        .and_then(Value::as_str)
        .unwrap_or("metric");
    let timestamp = payload
        .as_object()
        .and_then(|map| map.get("timestamp"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
    let file_name = format!(
        "{}_{}_{}_{}_{}.json",
        timestamp,
        sequence,
        sanitize_fragment(peer),
        sanitize_fragment(metric),
        sanitize_fragment(action),
    );
    let path = dir.join(file_name);
    let bytes = json::to_vec(&payload)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
    fs::write(&path, &bytes)?;
    Ok(path)
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct BridgeMetricKey {
    peer: String,
    metric: String,
    labels: Vec<(String, String)>,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct BridgeRemediationKey {
    peer: String,
    metric: String,
    labels: Vec<(String, String)>,
}

impl BridgeRemediationKey {
    fn from_event(event: &BridgeAnomalyEvent) -> Self {
        let mut labels: Vec<(String, String)> = event
            .labels
            .iter()
            .map(|label| (label.key.clone(), label.value.clone()))
            .collect();
        labels.sort();
        Self {
            peer: event.peer_id.clone(),
            metric: event.metric.clone(),
            labels,
        }
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("peer".to_string(), Value::String(self.peer.clone()));
        map.insert("metric".to_string(), Value::String(self.metric.clone()));
        let labels: Vec<Value> = self
            .labels
            .iter()
            .map(|(key, value)| {
                let mut label = Map::new();
                label.insert("key".to_string(), Value::String(key.clone()));
                label.insert("value".to_string(), Value::String(value.clone()));
                Value::Object(label)
            })
            .collect();
        map.insert("labels".to_string(), Value::Array(labels));
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let peer = object.get("peer").and_then(Value::as_str)?.to_string();
        let metric = object.get("metric").and_then(Value::as_str)?.to_string();
        let mut labels: Vec<(String, String)> = object
            .get("labels")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|entry| {
                        let label = entry.as_object()?;
                        let key = label.get("key")?.as_str()?;
                        let value = label.get("value")?.as_str()?;
                        Some((key.to_string(), value.to_string()))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        labels.sort();
        Some(Self {
            peer,
            metric,
            labels,
        })
    }
}

#[derive(Default, Debug)]
struct BridgeMetricState {
    last_value: Option<f64>,
    last_timestamp: Option<u64>,
    deltas: VecDeque<f64>,
    last_alert_ts: Option<u64>,
}

#[derive(Default, Debug)]
struct BridgeRemediationEntry {
    events: VecDeque<u64>,
    last_action: Option<BridgeRemediationActionType>,
    last_action_ts: Option<u64>,
}

impl BridgeRemediationEntry {
    fn record(&mut self, timestamp: u64) {
        self.events.push_back(timestamp);
        while let Some(front) = self.events.front().copied() {
            if timestamp.saturating_sub(front) > BRIDGE_REMEDIATION_WINDOW_SECS {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "events".to_string(),
            Value::Array(self.events.iter().copied().map(Value::from).collect()),
        );
        map.insert(
            "last_action".to_string(),
            self.last_action
                .map(|action| Value::String(action.as_str().to_string()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "last_action_ts".to_string(),
            self.last_action_ts.map(Value::from).unwrap_or(Value::Null),
        );
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let events = object
            .get("events")
            .and_then(Value::as_array)
            .map(|array| array.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
            .unwrap_or_default();
        let last_action = object
            .get("last_action")
            .and_then(Value::as_str)
            .and_then(BridgeRemediationActionType::from_str);
        let last_action_ts = match object.get("last_action_ts") {
            Some(Value::Null) | None => None,
            Some(value) => value.as_u64(),
        };
        let mut entry = Self {
            events: VecDeque::from(events),
            last_action,
            last_action_ts,
        };
        if let Some(last_ts) = entry.events.back().copied() {
            while let Some(front) = entry.events.front().copied() {
                if last_ts.saturating_sub(front) > BRIDGE_REMEDIATION_WINDOW_SECS {
                    entry.events.pop_front();
                } else {
                    break;
                }
            }
        }
        Some(entry)
    }
}

impl BridgeMetricState {
    fn reset(&mut self, value: f64, timestamp: u64) {
        self.last_value = Some(value);
        self.last_timestamp = Some(timestamp);
        self.deltas.clear();
        self.last_alert_ts = None;
    }

    fn record(&mut self, value: f64, delta: f64, timestamp: u64) {
        self.last_value = Some(value);
        self.last_timestamp = Some(timestamp);
        self.deltas.push_back(delta);
        while self.deltas.len() > BRIDGE_ANOMALY_WINDOW {
            self.deltas.pop_front();
        }
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "last_value".to_string(),
            self.last_value.map(Value::from).unwrap_or(Value::Null),
        );
        map.insert(
            "last_timestamp".to_string(),
            self.last_timestamp.map(Value::from).unwrap_or(Value::Null),
        );
        let deltas: Vec<_> = self
            .deltas
            .iter()
            .map(|delta| Value::from(*delta))
            .collect();
        map.insert("deltas".to_string(), Value::Array(deltas));
        map.insert(
            "last_alert_ts".to_string(),
            self.last_alert_ts.map(Value::from).unwrap_or(Value::Null),
        );
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let last_value = match object.get("last_value") {
            Some(Value::Null) | None => None,
            Some(v) => v.as_f64(),
        };
        let last_timestamp = match object.get("last_timestamp") {
            Some(Value::Null) | None => None,
            Some(v) => v.as_u64(),
        };
        let deltas = object
            .get("deltas")
            .and_then(Value::as_array)
            .map(|array| array.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
            .unwrap_or_default();
        let last_alert_ts = match object.get("last_alert_ts") {
            Some(Value::Null) | None => None,
            Some(v) => v.as_u64(),
        };
        let mut deque = VecDeque::from(deltas);
        while deque.len() > BRIDGE_ANOMALY_WINDOW {
            deque.pop_front();
        }
        Some(Self {
            last_value,
            last_timestamp,
            deltas: deque,
            last_alert_ts,
        })
    }
}

#[derive(Clone)]
struct BridgeMetricSample {
    metric: String,
    labels: Vec<(String, String)>,
    value: f64,
}

#[derive(Clone)]
struct BridgeMetricObservation {
    peer: String,
    metric: String,
    labels: Vec<(String, String)>,
    delta: f64,
    rate_per_sec: f64,
}

#[derive(Default)]
struct BridgeIngestResult {
    events: Vec<BridgeAnomalyEvent>,
    observations: Vec<BridgeMetricObservation>,
}

#[derive(Default)]
struct BridgeAnomalyDetector {
    metrics: HashMap<BridgeMetricKey, BridgeMetricState>,
    events: VecDeque<BridgeAnomalyEvent>,
}

#[derive(Clone, Copy, Debug)]
struct BridgeRemediationAckTiming {
    retry_after_secs: u64,
    escalate_after_secs: u64,
    max_retries: u32,
}

impl BridgeRemediationAckTiming {
    fn new(retry_after_secs: u64, escalate_after_secs: u64, max_retries: u32) -> Self {
        let escalate_after_secs = escalate_after_secs.max(retry_after_secs);
        Self {
            retry_after_secs,
            escalate_after_secs,
            max_retries,
        }
    }

    fn from_env_keys(
        retry_key: &str,
        escalate_key: &str,
        max_key: &str,
        fallback: BridgeRemediationAckTiming,
    ) -> (Self, bool) {
        let mut seen = false;

        let retry_after_secs = env::var(retry_key)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(|value| {
                seen = true;
                value
            })
            .unwrap_or(fallback.retry_after_secs);

        let escalate_after_secs = env::var(escalate_key)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(|value| {
                seen = true;
                value
            })
            .unwrap_or(fallback.escalate_after_secs);

        let max_retries = env::var(max_key)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| {
                seen = true;
                value
            })
            .unwrap_or(fallback.max_retries);

        (
            Self::new(retry_after_secs, escalate_after_secs, max_retries),
            seen,
        )
    }
}

impl Default for BridgeRemediationAckTiming {
    fn default() -> Self {
        Self::new(
            BRIDGE_REMEDIATION_ACK_RETRY_SECS,
            BRIDGE_REMEDIATION_ACK_ESCALATE_SECS,
            BRIDGE_REMEDIATION_ACK_MAX_RETRIES,
        )
    }
}

#[derive(Clone, Debug)]
struct BridgeRemediationAckPolicy {
    default: BridgeRemediationAckTiming,
    overrides: HashMap<BridgeRemediationPlaybook, BridgeRemediationAckTiming>,
}

impl BridgeRemediationAckPolicy {
    fn from_env() -> Self {
        let default = BridgeRemediationAckTiming::from_env_keys(
            ENV_REMEDIATION_ACK_RETRY_SECS,
            ENV_REMEDIATION_ACK_ESCALATE_SECS,
            ENV_REMEDIATION_ACK_MAX_RETRIES,
            BridgeRemediationAckTiming::default(),
        )
        .0;

        let mut overrides = HashMap::new();
        let base = default;

        let playbook_suffixes = [
            (BridgeRemediationPlaybook::None, "NONE"),
            (
                BridgeRemediationPlaybook::IncentiveThrottle,
                "INCENTIVE_THROTTLE",
            ),
            (
                BridgeRemediationPlaybook::GovernanceEscalation,
                "GOVERNANCE_ESCALATION",
            ),
        ];

        for (playbook, suffix) in playbook_suffixes {
            let retry_key = format!("{}_{}", ENV_REMEDIATION_ACK_RETRY_SECS, suffix);
            let escalate_key = format!("{}_{}", ENV_REMEDIATION_ACK_ESCALATE_SECS, suffix);
            let max_key = format!("{}_{}", ENV_REMEDIATION_ACK_MAX_RETRIES, suffix);
            let (timing, seen) = BridgeRemediationAckTiming::from_env_keys(
                &retry_key,
                &escalate_key,
                &max_key,
                base,
            );
            if seen {
                overrides.insert(playbook, timing);
            }
        }

        Self {
            default: base,
            overrides,
        }
    }

    fn timing_for(&self, playbook: BridgeRemediationPlaybook) -> BridgeRemediationAckTiming {
        self.overrides
            .get(&playbook)
            .copied()
            .unwrap_or(self.default)
    }
}

#[derive(Clone)]
struct BridgeAckLatencyObservation {
    playbook: BridgeRemediationPlaybook,
    state: BridgeDispatchAckState,
    latency: u64,
    count: u64,
}

#[derive(Default, Clone)]
struct AckLatencySeries {
    counts: BTreeMap<u64, u64>,
}

impl AckLatencySeries {
    fn observe(&mut self, latency: u64) {
        *self.counts.entry(latency).or_insert(0) += 1;
    }

    fn to_value(&self) -> Value {
        let items: Vec<Value> = self
            .counts
            .iter()
            .map(|(latency, count)| {
                let mut map = Map::new();
                map.insert("latency_seconds".to_string(), Value::from(*latency));
                map.insert("count".to_string(), Value::from(*count));
                Value::Object(map)
            })
            .collect();
        Value::Array(items)
    }

    fn restore(&mut self, value: &Value) {
        self.counts.clear();
        let Some(array) = value.as_array() else {
            return;
        };
        for entry in array {
            if let Some(object) = entry.as_object() {
                if let (Some(latency), Some(count)) = (
                    object.get("latency_seconds").and_then(Value::as_u64),
                    object.get("count").and_then(Value::as_u64),
                ) {
                    self.counts.insert(latency, count);
                }
            }
        }
    }
}

#[derive(Default, Clone)]
struct BridgeAckLatencyStore {
    series: HashMap<(BridgeRemediationPlaybook, BridgeDispatchAckState), AckLatencySeries>,
}

impl BridgeAckLatencyStore {
    fn observe(
        &mut self,
        playbook: BridgeRemediationPlaybook,
        state: BridgeDispatchAckState,
        latency: u64,
    ) {
        self.series
            .entry((playbook, state))
            .or_insert_with(AckLatencySeries::default)
            .observe(latency);
    }

    fn to_value(&self) -> Value {
        let entries: Vec<Value> = self
            .series
            .iter()
            .map(|((playbook, state), series)| {
                let mut map = Map::new();
                map.insert(
                    "playbook".to_string(),
                    Value::String(playbook.as_str().to_string()),
                );
                map.insert(
                    "state".to_string(),
                    Value::String(state.as_str().to_string()),
                );
                map.insert("samples".to_string(), series.to_value());
                Value::Object(map)
            })
            .collect();
        Value::Array(entries)
    }

    fn restore(&mut self, value: &Value) {
        self.series.clear();
        let Some(array) = value.as_array() else {
            return;
        };
        for entry in array {
            if let Some(object) = entry.as_object() {
                let Some(playbook_str) = object.get("playbook").and_then(Value::as_str) else {
                    continue;
                };
                let Some(state_str) = object.get("state").and_then(Value::as_str) else {
                    continue;
                };
                let Some(series_value) = object.get("samples") else {
                    continue;
                };
                if let (Some(playbook), Some(state)) = (
                    BridgeRemediationPlaybook::from_str(playbook_str),
                    BridgeDispatchAckState::from_str(state_str),
                ) {
                    let mut series = AckLatencySeries::default();
                    series.restore(series_value);
                    if !series.counts.is_empty() {
                        self.series.insert((playbook, state), series);
                    }
                }
            }
        }
    }

    fn observations(&self) -> Vec<BridgeAckLatencyObservation> {
        let mut out = Vec::new();
        for ((playbook, state), series) in &self.series {
            for (latency, count) in &series.counts {
                out.push(BridgeAckLatencyObservation {
                    playbook: *playbook,
                    state: *state,
                    latency: *latency,
                    count: *count,
                });
            }
        }
        out
    }
}

struct BridgeDispatchUpdate {
    action: BridgeRemediationAction,
    ack_sample: Option<BridgeAckLatencyObservation>,
}

impl Default for BridgeRemediationAckPolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

struct BridgeRemediationEngine {
    entries: HashMap<BridgeRemediationKey, BridgeRemediationEntry>,
    actions: VecDeque<BridgeRemediationAction>,
    ack_policy: BridgeRemediationAckPolicy,
    ack_latency: BridgeAckLatencyStore,
}

impl Default for BridgeRemediationEngine {
    fn default() -> Self {
        Self::new(BridgeRemediationAckPolicy::from_env())
    }
}

impl BridgeRemediationEngine {
    fn new(policy: BridgeRemediationAckPolicy) -> Self {
        let engine = Self {
            entries: HashMap::new(),
            actions: VecDeque::new(),
            ack_policy: policy,
            ack_latency: BridgeAckLatencyStore::default(),
        };
        engine.update_ack_targets();
        engine
    }

    fn update_ack_targets(&self) {
        let metrics = aggregator_metrics();
        for playbook in BridgeRemediationPlaybook::variants() {
            let timing = self.ack_policy.timing_for(*playbook);
            metrics
                .bridge_remediation_ack_target_seconds
                .with_label_values(&[playbook.as_str(), "retry"])
                .set(timing.retry_after_secs as f64);
            metrics
                .bridge_remediation_ack_target_seconds
                .with_label_values(&[playbook.as_str(), "escalate"])
                .set(timing.escalate_after_secs as f64);
        }
    }

    fn ack_latency_observations(&self) -> Vec<BridgeAckLatencyObservation> {
        self.ack_latency.observations()
    }

    fn ingest(&mut self, event: &BridgeAnomalyEvent) -> Option<BridgeRemediationAction> {
        if event.labels.is_empty() {
            return None;
        }
        let key = BridgeRemediationKey::from_event(event);
        let entry = self
            .entries
            .entry(key)
            .or_insert_with(BridgeRemediationEntry::default);
        entry.record(event.timestamp);
        let occurrences = entry.events.len();
        let ratio = if event.threshold > 0.0 {
            event.delta / event.threshold
        } else {
            0.0
        };
        let action = if occurrences >= BRIDGE_REMEDIATION_ESCALATE_COUNT
            || event.delta >= BRIDGE_REMEDIATION_ESCALATE_DELTA
            || ratio >= BRIDGE_REMEDIATION_ESCALATE_RATIO
        {
            Some(BridgeRemediationActionType::Escalate)
        } else if occurrences >= BRIDGE_REMEDIATION_QUARANTINE_COUNT
            || event.delta >= BRIDGE_REMEDIATION_QUARANTINE_DELTA
            || ratio >= BRIDGE_REMEDIATION_QUARANTINE_RATIO
        {
            Some(BridgeRemediationActionType::Quarantine)
        } else if occurrences >= BRIDGE_REMEDIATION_THROTTLE_COUNT
            || event.delta >= BRIDGE_REMEDIATION_THROTTLE_DELTA
            || ratio >= BRIDGE_REMEDIATION_THROTTLE_RATIO
        {
            Some(BridgeRemediationActionType::Throttle)
        } else if event.delta >= BRIDGE_REMEDIATION_PAGE_DELTA
            || ratio >= BRIDGE_REMEDIATION_PAGE_RATIO
        {
            Some(BridgeRemediationActionType::Page)
        } else {
            None
        };
        let Some(action_type) = action else {
            return None;
        };

        let emit = match entry.last_action {
            Some(prev) if action_type < prev => false,
            Some(prev) if action_type == prev => {
                let last_ts = entry.last_action_ts.unwrap_or(0);
                event.timestamp.saturating_sub(last_ts) >= BRIDGE_REMEDIATION_PAGE_COOLDOWN_SECS
            }
            _ => true,
        };

        if !emit {
            return None;
        }

        entry.last_action = Some(action_type);
        entry.last_action_ts = Some(event.timestamp);

        let action = BridgeRemediationAction::new(event, action_type, occurrences, ratio);
        self.actions.push_back(action.clone());
        while self.actions.len() > BRIDGE_REMEDIATION_MAX_ACTIONS {
            self.actions.pop_front();
        }
        Some(action)
    }

    fn record_dispatch_attempt(
        &mut self,
        action: &BridgeRemediationAction,
        ack: Option<&BridgeDispatchAckRecord>,
        dispatched_at: u64,
        status: &str,
    ) -> Option<BridgeDispatchUpdate> {
        let mut updated = None;
        for stored in self.actions.iter_mut().rev() {
            if stored.peer_id == action.peer_id
                && stored.metric == action.metric
                && stored.timestamp == action.timestamp
                && stored.action == action.action
            {
                stored.dispatch_attempts = stored.dispatch_attempts.saturating_add(1);
                stored.last_dispatch_at = Some(dispatched_at);
                stored.first_dispatch_at.get_or_insert(dispatched_at);
                if ack.is_some() || status == "success" {
                    stored.pending_since.get_or_insert(dispatched_at);
                }
                let mut sample = None;
                if let Some(ack) = ack {
                    let ack_state = ack.state;
                    stored.last_ack_state = Some(ack.state);
                    if let Some(notes) = ack.notes.as_ref() {
                        stored.last_ack_notes = Some(notes.clone());
                    }
                    if ack.is_completion() {
                        if ack.closed && stored.closed_out_at.is_none() {
                            stored.closed_out_at = Some(ack.timestamp);
                        }
                        if ack.acknowledged && stored.acknowledged_at.is_none() {
                            stored.acknowledged_at = Some(ack.timestamp);
                        }
                        if let Some(notes) = ack.notes.as_ref() {
                            stored.acknowledgement_notes = Some(notes.clone());
                        }
                        stored.pending_since = None;
                        stored.pending_escalated = false;
                        stored.last_ack_notes = ack.notes.clone();
                        stored.follow_up_notes = None;
                        stored.auto_retry_count = 0;
                        stored.last_auto_retry_at = None;
                        if let Some(first_dispatch_at) = stored.first_dispatch_at {
                            let latency = ack.timestamp.saturating_sub(first_dispatch_at);
                            self.ack_latency
                                .observe(stored.playbook, ack_state, latency);
                            sample = Some(BridgeAckLatencyObservation {
                                playbook: stored.playbook,
                                state: ack_state,
                                latency,
                                count: 1,
                            });
                        }
                    }
                }
                updated = Some(BridgeDispatchUpdate {
                    action: stored.clone(),
                    ack_sample: sample,
                });
                break;
            }
        }
        updated
    }

    fn pending_followups(&mut self, now: u64) -> Vec<BridgeRemediationFollowUp> {
        let mut followups = Vec::new();
        let mut escalations = Vec::new();
        for stored in self.actions.iter_mut() {
            if stored.acknowledged_at.is_some() || stored.closed_out_at.is_some() {
                continue;
            }
            if stored.dispatch_attempts == 0 {
                continue;
            }
            let timing = self.ack_policy.timing_for(stored.playbook);
            let pending_since = stored
                .pending_since
                .or(stored.first_dispatch_at)
                .unwrap_or(stored.timestamp);
            let elapsed = now.saturating_sub(pending_since);
            let retry_due = stored
                .last_dispatch_at
                .map(|last| now.saturating_sub(last) >= timing.retry_after_secs)
                .unwrap_or(false);
            let retry_window_ok = stored
                .last_auto_retry_at
                .map(|last| now.saturating_sub(last) >= timing.retry_after_secs)
                .unwrap_or(true);

            if elapsed >= timing.escalate_after_secs
                && !stored.pending_escalated
                && stored.action != BridgeRemediationActionType::Escalate
            {
                let escalation = BridgeRemediationAction {
                    peer_id: stored.peer_id.clone(),
                    metric: stored.metric.clone(),
                    labels: stored.labels.clone(),
                    action: BridgeRemediationActionType::Escalate,
                    playbook: BridgeRemediationPlaybook::GovernanceEscalation,
                    occurrences: stored.occurrences,
                    delta: stored.delta,
                    threshold: stored.threshold,
                    ratio: stored.ratio,
                    timestamp: now,
                    acknowledged_at: None,
                    closed_out_at: None,
                    acknowledgement_notes: None,
                    first_dispatch_at: None,
                    last_dispatch_at: None,
                    dispatch_attempts: 0,
                    auto_retry_count: 0,
                    last_auto_retry_at: None,
                    pending_since: None,
                    pending_escalated: false,
                    last_ack_state: None,
                    last_ack_notes: None,
                    follow_up_notes: Some(format!(
                        "Automated escalation after {}s without closure ({} attempts)",
                        elapsed, stored.dispatch_attempts
                    )),
                };
                stored.pending_escalated = true;
                let previous_notes = stored.follow_up_notes.take();
                stored.follow_up_notes = Some(match previous_notes {
                    Some(existing) if !existing.is_empty() => format!(
                        "{existing}; escalation queued after {}s without closure",
                        elapsed
                    ),
                    _ => format!(
                        "Automated escalation queued after {}s without closure",
                        elapsed
                    ),
                });
                escalations.push(escalation.clone());
                followups.push(BridgeRemediationFollowUp::Escalate { escalation });
                continue;
            }

            if timing.max_retries == 0 {
                continue;
            }

            if elapsed >= timing.retry_after_secs
                && retry_due
                && retry_window_ok
                && stored.auto_retry_count < timing.max_retries
            {
                stored.auto_retry_count = stored.auto_retry_count.saturating_add(1);
                stored.last_auto_retry_at = Some(now);
                let previous_notes = stored.follow_up_notes.take();
                stored.follow_up_notes = Some(match previous_notes {
                    Some(existing) if !existing.is_empty() => format!(
                        "{existing}; retry {} after {}s without acknowledgement",
                        stored.auto_retry_count, elapsed
                    ),
                    _ => format!(
                        "Automated retry {} after {}s without acknowledgement",
                        stored.auto_retry_count, elapsed
                    ),
                });
                followups.push(BridgeRemediationFollowUp::Retry {
                    action: stored.clone(),
                });
            }
        }
        for escalation in escalations {
            self.actions.push_back(escalation);
            while self.actions.len() > BRIDGE_REMEDIATION_MAX_ACTIONS {
                self.actions.pop_front();
            }
        }
        followups
    }

    fn snapshot(&self) -> Value {
        let mut map = Map::new();
        let entries = self
            .entries
            .iter()
            .map(|(key, entry)| {
                let mut item = Map::new();
                item.insert("key".to_string(), key.to_value());
                item.insert("entry".to_string(), entry.to_value());
                Value::Object(item)
            })
            .collect();
        map.insert("entries".to_string(), Value::Array(entries));
        map.insert(
            "actions".to_string(),
            Value::Array(
                self.actions
                    .iter()
                    .map(BridgeRemediationAction::to_value)
                    .collect(),
            ),
        );
        map.insert("ack_latency".to_string(), self.ack_latency.to_value());
        Value::Object(map)
    }

    fn restore(&mut self, value: &Value) {
        self.entries.clear();
        self.actions.clear();
        self.ack_latency = BridgeAckLatencyStore::default();
        let Some(object) = value.as_object() else {
            return;
        };
        if let Some(entries) = object.get("entries").and_then(Value::as_array) {
            for entry in entries {
                if let Some(entry_obj) = entry.as_object() {
                    if let (Some(key_value), Some(entry_value)) =
                        (entry_obj.get("key"), entry_obj.get("entry"))
                    {
                        if let (Some(key), Some(entry_state)) = (
                            BridgeRemediationKey::from_value(key_value),
                            BridgeRemediationEntry::from_value(entry_value),
                        ) {
                            self.entries.insert(key, entry_state);
                        }
                    }
                }
            }
        }
        if let Some(actions) = object.get("actions").and_then(Value::as_array) {
            for action_value in actions {
                if let Some(action) = BridgeRemediationAction::from_value(action_value) {
                    self.actions.push_back(action);
                }
            }
        }
        while self.actions.len() > BRIDGE_REMEDIATION_MAX_ACTIONS {
            self.actions.pop_front();
        }
        if let Some(latency_value) = object.get("ack_latency") {
            self.ack_latency.restore(latency_value);
        }
        self.update_ack_targets();
    }

    fn actions(&self) -> Vec<BridgeRemediationAction> {
        self.actions.iter().cloned().collect()
    }
}

impl BridgeAnomalyDetector {
    fn ingest(&mut self, peer_id: &str, metrics: &Value, timestamp: u64) -> BridgeIngestResult {
        let mut triggered = Vec::new();
        let mut observations = Vec::new();
        for sample in collect_bridge_metric_samples(metrics) {
            if !BRIDGE_MONITORED_COUNTERS.contains(&sample.metric.as_str()) {
                continue;
            }
            let key = BridgeMetricKey {
                peer: peer_id.to_string(),
                metric: sample.metric.clone(),
                labels: sample.labels.clone(),
            };
            let (event, observation) = self.observe(key, sample.value, timestamp);
            if let Some(event) = event {
                triggered.push(event.clone());
                self.push_event(event);
            }
            if let Some(observation) = observation {
                observations.push(observation);
            }
        }
        BridgeIngestResult {
            events: triggered,
            observations,
        }
    }

    fn observe(
        &mut self,
        key: BridgeMetricKey,
        value: f64,
        timestamp: u64,
    ) -> (Option<BridgeAnomalyEvent>, Option<BridgeMetricObservation>) {
        if !value.is_finite() {
            return (None, None);
        }
        let state = self.metrics.entry(key.clone()).or_default();
        match (state.last_value, state.last_timestamp) {
            (None, _) | (_, None) => {
                state.reset(value, timestamp);
                (None, None)
            }
            (Some(previous), Some(previous_timestamp)) => {
                let mut delta = value - previous;
                if delta < -COUNTER_EPSILON {
                    state.reset(value, timestamp);
                    return (None, None);
                }
                if delta < 0.0 {
                    delta = 0.0;
                }
                let elapsed = timestamp.saturating_sub(previous_timestamp).max(1);
                let rate = delta / elapsed as f64;
                let window_len = state.deltas.len();
                let mut anomaly = None;
                if window_len >= BRIDGE_ANOMALY_BASELINE_MIN {
                    let sum: f64 = state.deltas.iter().sum();
                    let mean = sum / window_len as f64;
                    let variance_sum: f64 = state
                        .deltas
                        .iter()
                        .map(|sample| {
                            let diff = *sample - mean;
                            diff * diff
                        })
                        .sum();
                    let variance = variance_sum / window_len as f64;
                    let stddev = variance.sqrt();
                    let baseline_std = stddev.max(BRIDGE_ANOMALY_MIN_STDDEV);
                    let threshold = mean + baseline_std * BRIDGE_ANOMALY_STD_MULTIPLIER;
                    let cooldown_ok = state
                        .last_alert_ts
                        .map(|last| timestamp.saturating_sub(last) >= BRIDGE_ANOMALY_COOLDOWN_SECS)
                        .unwrap_or(true);
                    if delta >= BRIDGE_ANOMALY_MIN_DELTA && delta >= threshold && cooldown_ok {
                        state.last_alert_ts = Some(timestamp);
                        let labels = key
                            .labels
                            .iter()
                            .map(|(k, v)| BridgeAnomalyLabel {
                                key: k.clone(),
                                value: v.clone(),
                            })
                            .collect();
                        anomaly = Some(BridgeAnomalyEvent {
                            metric: key.metric.clone(),
                            peer_id: key.peer.clone(),
                            labels,
                            delta,
                            mean,
                            stddev,
                            threshold,
                            window: window_len,
                            timestamp,
                        });
                    }
                }
                state.record(value, delta, timestamp);
                let observation = BridgeMetricObservation {
                    peer: key.peer.clone(),
                    metric: key.metric.clone(),
                    labels: key.labels.clone(),
                    delta,
                    rate_per_sec: rate,
                };
                (anomaly, Some(observation))
            }
        }
    }

    fn push_event(&mut self, event: BridgeAnomalyEvent) {
        self.events.push_back(event);
        while self.events.len() > BRIDGE_ANOMALY_MAX_EVENTS {
            self.events.pop_front();
        }
    }

    fn events(&self) -> Vec<BridgeAnomalyEvent> {
        self.events.iter().cloned().collect()
    }

    fn snapshot(&self) -> Value {
        let mut metrics = Vec::new();
        for (key, state) in &self.metrics {
            let mut entry = Map::new();
            entry.insert("peer".to_string(), Value::String(key.peer.clone()));
            entry.insert("metric".to_string(), Value::String(key.metric.clone()));
            entry.insert(
                "labels".to_string(),
                encode_bridge_metric_labels(&key.labels),
            );
            entry.insert("state".to_string(), state.to_value());
            metrics.push(Value::Object(entry));
        }
        let events = self
            .events
            .iter()
            .map(BridgeAnomalyEvent::to_value)
            .collect();
        let mut map = Map::new();
        map.insert("metrics".to_string(), Value::Array(metrics));
        map.insert("events".to_string(), Value::Array(events));
        Value::Object(map)
    }

    fn restore(&mut self, snapshot: &Value) {
        self.metrics.clear();
        self.events.clear();
        let Some(object) = snapshot.as_object() else {
            return;
        };
        if let Some(metrics) = object.get("metrics").and_then(Value::as_array) {
            for entry in metrics {
                let Some(metric_obj) = entry.as_object() else {
                    continue;
                };
                let Some(peer) = metric_obj.get("peer").and_then(Value::as_str) else {
                    continue;
                };
                let Some(metric) = metric_obj.get("metric").and_then(Value::as_str) else {
                    continue;
                };
                let labels = metric_obj
                    .get("labels")
                    .map(decode_bridge_metric_labels)
                    .unwrap_or_default();
                let Some(state_value) = metric_obj.get("state") else {
                    continue;
                };
                let Some(state) = BridgeMetricState::from_value(state_value) else {
                    continue;
                };
                let key = BridgeMetricKey {
                    peer: peer.to_string(),
                    metric: metric.to_string(),
                    labels,
                };
                self.metrics.insert(key, state);
            }
        }
        if let Some(events) = object.get("events").and_then(Value::as_array) {
            let mut deque = VecDeque::new();
            for entry in events {
                if let Some(event) = BridgeAnomalyEvent::from_value(entry) {
                    deque.push_back(event);
                }
            }
            while deque.len() > BRIDGE_ANOMALY_MAX_EVENTS {
                deque.pop_front();
            }
            self.events = deque;
        }
    }
}

fn encode_bridge_metric_labels(labels: &[(String, String)]) -> Value {
    let mut entries: Vec<_> = labels.iter().cloned().collect();
    entries.sort();
    let array = entries
        .into_iter()
        .map(|(key, value)| {
            let mut map = Map::new();
            map.insert("key".to_string(), Value::String(key));
            map.insert("value".to_string(), Value::String(value));
            Value::Object(map)
        })
        .collect();
    Value::Array(array)
}

fn decode_bridge_metric_labels(value: &Value) -> Vec<(String, String)> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    for entry in array {
        if let Some(object) = entry.as_object() {
            if let (Some(key), Some(val)) = (
                object.get("key").and_then(Value::as_str),
                object.get("value").and_then(Value::as_str),
            ) {
                labels.push((key.to_string(), val.to_string()));
            }
        }
    }
    labels.sort();
    labels
}

fn collect_bridge_metric_samples(metrics: &Value) -> Vec<BridgeMetricSample> {
    let mut out = Vec::new();
    let root = match metrics {
        Value::Object(map) => map,
        _ => return out,
    };
    for &metric in &BRIDGE_MONITORED_COUNTERS {
        if let Some(value) = root.get(metric) {
            collect_metric_samples(metric, value, &mut out);
        }
    }
    let mut dedup: HashMap<(String, Vec<(String, String)>), f64> = HashMap::new();
    for sample in out {
        dedup.insert((sample.metric.clone(), sample.labels.clone()), sample.value);
    }
    dedup
        .into_iter()
        .map(|((metric, labels), value)| BridgeMetricSample {
            metric,
            labels,
            value,
        })
        .collect()
}

fn collect_metric_samples(metric: &str, value: &Value, out: &mut Vec<BridgeMetricSample>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_metric_samples(metric, item, out);
            }
        }
        Value::Object(map) => {
            if let Some(samples) = map.get("samples") {
                collect_metric_samples(metric, samples, out);
            }
            let counter = map
                .get("value")
                .and_then(Value::as_f64)
                .or_else(|| map.get("counter").and_then(Value::as_f64));
            if let Some(counter) = counter {
                let labels = extract_metric_labels(map);
                out.push(BridgeMetricSample {
                    metric: metric.to_string(),
                    labels,
                    value: counter,
                });
            }
            for (key, child) in map {
                if matches!(key.as_str(), "labels" | "samples" | "value" | "counter") {
                    continue;
                }
                if matches!(child, Value::Array(_) | Value::Object(_)) {
                    collect_metric_samples(metric, child, out);
                }
            }
        }
        _ => {}
    }
}

fn extract_metric_labels(map: &Map) -> Vec<(String, String)> {
    let mut labels = BTreeMap::new();
    if let Some(label_map) = map.get("labels").and_then(Value::as_object) {
        for (key, value) in label_map {
            if let Some(rendered) = label_value(value) {
                labels.insert(key.clone(), rendered);
            }
        }
    }
    for key in ["asset", "result", "reason", "kind", "outcome"] {
        if let Some(value) = map.get(key).and_then(label_value) {
            labels.entry(key.to_string()).or_insert(value);
        }
    }
    labels.into_iter().collect()
}

fn label_value(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(v) = value.as_i64() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_u64() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_f64() {
        if v.is_finite() {
            return Some(v.to_string());
        }
    }
    if let Some(v) = value.as_bool() {
        return Some(v.to_string());
    }
    None
}

#[cfg(feature = "s3")]
static S3_BUCKET: Lazy<Option<String>> = Lazy::new(|| std::env::var("S3_BUCKET").ok());

fn merge(a: &mut Value, b: &Value) {
    match b {
        Value::Object(bm) => {
            if !a.is_object() {
                *a = Value::Object(Map::new());
            }
            if let Some(am) = a.as_object_mut() {
                for (k, bv) in bm {
                    merge(am.entry(k.clone()).or_insert(Value::Null), bv);
                }
            }
        }
        Value::Number(bn) => {
            let sum = a.as_f64().unwrap_or(0.0) + bn.as_f64();
            *a = Value::from(Number::from(sum));
        }
        _ => {
            *a = b.clone();
        }
    }
}

fn collect_correlations(value: &Value) -> Vec<RawCorrelation> {
    fn walk(value: &Value, metric: Option<&str>, out: &mut Vec<RawCorrelation>) {
        match value {
            Value::Object(map) => {
                if let Some(correlation) = map
                    .get("correlation_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    let metric_name = metric.unwrap_or("unknown").to_string();
                    let val = map.get("value").and_then(|v| v.as_f64());
                    out.push(RawCorrelation {
                        metric: metric_name,
                        correlation_id: correlation.to_string(),
                        value: val,
                    });
                }
                if let Some(labels) = map.get("labels").and_then(|v| v.as_object()) {
                    if let Some(correlation) = labels
                        .get("correlation_id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        let metric_name = metric.unwrap_or("unknown").to_string();
                        let val = map.get("value").and_then(|v| v.as_f64());
                        out.push(RawCorrelation {
                            metric: metric_name,
                            correlation_id: correlation.to_string(),
                            value: val,
                        });
                    }
                }
                for (k, v) in map {
                    let next_metric = metric.or_else(|| Some(k.as_str()));
                    walk(v, next_metric, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, metric, out);
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    walk(value, None, &mut out);
    let mut seen = HashSet::new();
    out.into_iter()
        .filter(|entry| seen.insert((entry.metric.clone(), entry.correlation_id.clone())))
        .collect()
}

fn quantize_counter(value: f64) -> Option<u64> {
    if !value.is_finite() {
        return None;
    }
    if value < COUNTER_EPSILON {
        return Some(0);
    }
    let rounded = value.round();
    if (rounded - value).abs() > COUNTER_EPSILON {
        None
    } else if rounded < 0.0 {
        None
    } else {
        Some(rounded as u64)
    }
}

fn extract_tls_warning_counters(metrics: &Value) -> Vec<(String, String, f64)> {
    extract_tls_warning_metric(metrics, "tls_env_warning_total")
}

fn extract_tls_warning_detail_fingerprints(metrics: &Value) -> Vec<TlsFingerprintSample> {
    extract_tls_warning_fingerprint_metric(metrics, "tls_env_warning_detail_fingerprint")
}

fn extract_tls_warning_variables_fingerprints(metrics: &Value) -> Vec<TlsFingerprintSample> {
    extract_tls_warning_fingerprint_metric(metrics, "tls_env_warning_variables_fingerprint")
}

fn extract_tls_warning_metric(metrics: &Value, key: &str) -> Vec<(String, String, f64)> {
    let mut samples = Vec::new();
    let root = match metrics {
        Value::Object(map) => map.get(key),
        _ => None,
    };
    if let Some(value) = root {
        collect_tls_warning_samples(value, &mut samples);
    }

    let mut dedup = HashMap::new();
    for (prefix, code, value) in samples {
        dedup.insert((prefix, code), value);
    }
    dedup
        .into_iter()
        .map(|((prefix, code), value)| (prefix, code, value))
        .collect()
}

fn extract_tls_warning_fingerprint_metric(metrics: &Value, key: &str) -> Vec<TlsFingerprintSample> {
    let mut samples = Vec::new();
    let root = match metrics {
        Value::Object(map) => map.get(key),
        _ => None,
    };
    if let Some(value) = root {
        collect_tls_warning_fingerprint_samples(value, &mut samples);
    }

    let mut dedup: HashMap<(String, String), TlsFingerprintSample> = HashMap::new();
    for sample in samples {
        let key = (sample.prefix.clone(), sample.code.clone());
        dedup
            .entry(key)
            .and_modify(|existing| match (&existing.value, &sample.value) {
                (TlsFingerprintValue::Parsed(_), TlsFingerprintValue::Parsed(_)) => {
                    *existing = sample.clone();
                }
                (TlsFingerprintValue::Parsed(_), TlsFingerprintValue::Invalid(_)) => {}
                (TlsFingerprintValue::Invalid(_), TlsFingerprintValue::Parsed(_)) => {
                    *existing = sample.clone();
                }
                (TlsFingerprintValue::Invalid(_), TlsFingerprintValue::Invalid(_)) => {
                    *existing = sample.clone();
                }
            })
            .or_insert(sample);
    }

    dedup.into_values().collect()
}

fn collect_tls_warning_samples(value: &Value, out: &mut Vec<(String, String, f64)>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tls_warning_samples(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(samples) = map.get("samples") {
                collect_tls_warning_samples(samples, out);
            }

            let labels = map.get("labels").and_then(|labels| labels.as_object());
            let prefix = labels
                .and_then(|obj| obj.get("prefix"))
                .and_then(|v| v.as_str())
                .or_else(|| map.get("prefix").and_then(|v| v.as_str()));
            let code = labels
                .and_then(|obj| obj.get("code"))
                .and_then(|v| v.as_str())
                .or_else(|| map.get("code").and_then(|v| v.as_str()));
            let value_field = map
                .get("value")
                .and_then(|v| v.as_f64())
                .or_else(|| map.get("counter").and_then(|v| v.as_f64()));
            if let (Some(prefix), Some(code), Some(counter)) = (prefix, code, value_field) {
                out.push((prefix.to_string(), code.to_string(), counter));
            }

            for child in map.values() {
                if matches!(child, Value::Array(_) | Value::Object(_)) {
                    collect_tls_warning_samples(child, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_tls_warning_fingerprint_samples(value: &Value, out: &mut Vec<TlsFingerprintSample>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tls_warning_fingerprint_samples(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(samples) = map.get("samples") {
                collect_tls_warning_fingerprint_samples(samples, out);
            }

            let labels = map.get("labels").and_then(|labels| labels.as_object());
            let prefix = labels
                .and_then(|obj| obj.get("prefix"))
                .and_then(|v| v.as_str())
                .or_else(|| map.get("prefix").and_then(|v| v.as_str()));
            let code = labels
                .and_then(|obj| obj.get("code"))
                .and_then(|v| v.as_str())
                .or_else(|| map.get("code").and_then(|v| v.as_str()));

            if let (Some(prefix), Some(code)) = (prefix, code) {
                if let Some(value) = fingerprint_value_from_map(map) {
                    out.push(TlsFingerprintSample {
                        prefix: prefix.to_string(),
                        code: code.to_string(),
                        value,
                    });
                }
            }

            for (key, child) in map {
                if key == "labels" || key == "samples" {
                    continue;
                }
                if matches!(child, Value::Array(_) | Value::Object(_)) {
                    collect_tls_warning_fingerprint_samples(child, out);
                }
            }
        }
        _ => {}
    }
}

fn fingerprint_value_from_map(map: &Map) -> Option<TlsFingerprintValue> {
    let raw = map.get("value").or_else(|| map.get("counter"))?;
    match parse_fingerprint_number(raw) {
        Ok(parsed) => Some(TlsFingerprintValue::Parsed(parsed)),
        Err(raw) => Some(TlsFingerprintValue::Invalid(raw)),
    }
}

fn parse_fingerprint_number(value: &Value) -> Result<i64, String> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int);
            }
            if let Some(uint) = number.as_u64() {
                if let Ok(int) = i64::try_from(uint) {
                    return Ok(int);
                }
            }
            let float = number.as_f64();
            if !float.is_finite() {
                return Err(number_to_display(number));
            }
            let rounded = float.round();
            if (rounded - float).abs() > COUNTER_EPSILON {
                return Err(number_to_display(number));
            }
            if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
                return Err(number_to_display(number));
            }
            Ok(rounded as i64)
        }
        Value::String(text) => parse_string_fingerprint(text).ok_or_else(|| text.clone()),
        other => Err(other.to_string()),
    }
}

fn parse_string_fingerprint(value: &str) -> Option<i64> {
    if let Ok(parsed) = value.parse::<i64>() {
        return Some(parsed);
    }
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    if stripped.len() != 16 {
        return None;
    }
    let mut acc = 0u64;
    for ch in stripped.chars() {
        let digit = ch.to_digit(16)? as u64;
        acc = (acc << 4) | digit;
    }
    Some(i64::from_le_bytes(acc.to_le_bytes()))
}

fn number_to_display(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        value.to_string()
    } else if let Some(value) = number.as_u64() {
        value.to_string()
    } else {
        format_float_for_logging(number.as_f64())
    }
}

fn format_float_for_logging(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let mut formatted = format!("{value:.6}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted.is_empty() {
        formatted.push('0');
    }
    formatted
}

fn extract_tls_warning_last_seen(metrics: &Value) -> Vec<(String, String, f64)> {
    let mut samples = Vec::new();
    let root = match metrics {
        Value::Object(map) => map.get("tls_env_warning_last_seen_seconds"),
        _ => None,
    };
    if let Some(value) = root {
        collect_tls_warning_samples(value, &mut samples);
    }

    let mut dedup = HashMap::new();
    for (prefix, code, value) in samples {
        dedup
            .entry((prefix.clone(), code.clone()))
            .and_modify(|existing| {
                if value > *existing {
                    *existing = value;
                }
            })
            .or_insert(value);
    }
    dedup
        .into_iter()
        .map(|((prefix, code), value)| (prefix, code, value))
        .collect()
}

fn spawn_log_dump(record: CorrelationRecord) {
    let api = std::env::var("TB_LOG_API_URL").ok();
    let db = std::env::var("TB_LOG_DB_PATH").ok();
    let dump_dir = std::env::var("TB_LOG_DUMP_DIR").unwrap_or_else(|_| "log_dumps".into());
    if let (Some(api), Some(db)) = (api, db) {
        spawn(async move {
            if let Err(err) = fetch_and_dump_logs(api, db, dump_dir.clone(), record.clone()).await {
                warn!(
                    target: "aggregator",
                    error = %err,
                    correlation = %record.correlation_id,
                    "log dump failed"
                );
            }
        });
    } else {
        warn!(
            target: "aggregator",
            correlation = %record.correlation_id,
            "log dump skipped; log API configuration missing"
        );
    }
}

async fn fetch_and_dump_logs(
    api: String,
    db: String,
    dump_dir: String,
    record: CorrelationRecord,
) -> Result<(), String> {
    let client = http_client();
    let base = api.trim_end_matches('/');
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("db", &db);
    serializer.append_pair("correlation", &record.correlation_id);
    serializer.append_pair("limit", "50");
    let url = format!("{}/logs/search?{}", base, serializer.finish());
    let response = client
        .request(Method::Get, &url)
        .map_err(|e| format!("request build failed: {e}"))?
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("http status {}", response.status().as_u16()));
    }
    let body = response
        .text()
        .map_err(|e| format!("body read failed: {e}"))?;
    let path = persist_log_dump(&dump_dir, &record, &body)
        .await
        .map_err(|e| format!("persist failed: {e}"))?;
    info!(
        target: "aggregator",
        correlation = %record.correlation_id,
        metric = %record.metric,
        path = %path.display(),
        "wrote correlated log dump"
    );
    Ok(())
}

async fn persist_log_dump(
    dump_dir: &str,
    record: &CorrelationRecord,
    body: &str,
) -> io::Result<PathBuf> {
    let dir = dump_dir.to_string();
    let record = record.clone();
    let payload = body.as_bytes().to_vec();
    spawn_blocking(move || -> io::Result<PathBuf> {
        let dir_path = Path::new(&dir);
        std::fs::create_dir_all(dir_path)?;
        let file_name = format!(
            "{}_{}_{}_{}.json",
            sanitize_fragment(&record.metric),
            sanitize_fragment(&record.peer_id),
            sanitize_fragment(&record.correlation_id),
            record.timestamp
        );
        let path = dir_path.join(file_name);
        std::fs::write(&path, &payload)?;
        Ok(path)
    })
    .await
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
}

fn sanitize_fragment(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

async fn ingest(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let token = state.current_token();
    let authorized = request
        .header("x-auth-token")
        .map(|value| value == token)
        .unwrap_or(false);
    if !authorized {
        return Ok(Response::new(StatusCode::UNAUTHORIZED));
    }

    warn!(target: "aggregator", "ingest request received");

    let payload = parse_peer_stats(request.body_bytes())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| HttpError::Handler(format!("clock error: {err}")))?
        .as_secs();

    {
        let mut map = state.data.lock().unwrap();
        for stat in &payload {
            let entry = map
                .entry(stat.peer_id.clone())
                .or_insert_with(VecDeque::new);
            if let Some((ts, last)) = entry.back_mut() {
                if *ts == now {
                    merge(last, &stat.metrics);
                    let value =
                        json::to_vec(entry).map_err(|err| HttpError::Handler(err.to_string()))?;
                    let _ = state
                        .store
                        .put_bytes(METRICS_CF, stat.peer_id.as_bytes(), &value);
                    for raw in collect_correlations(&stat.metrics) {
                        let record = CorrelationRecord {
                            metric: raw.metric.clone(),
                            correlation_id: raw.correlation_id.clone(),
                            peer_id: stat.peer_id.clone(),
                            value: raw.value,
                            timestamp: now,
                        };
                        state.record_correlation(&raw.metric, record.clone());
                        if raw.metric == "quic_handshake_fail_total" {
                            state.handle_quic_failure(&record);
                        }
                    }
                    state.record_tls_warning_samples(&stat.peer_id, &stat.metrics);
                    state.record_bridge_anomalies(&stat.peer_id, &stat.metrics, now);
                    continue;
                }
            }
            entry.push_back((now, stat.metrics.clone()));
            if entry.len() > 1024 {
                entry.pop_front();
            }
            let value = json::to_vec(entry).map_err(|err| HttpError::Handler(err.to_string()))?;
            let _ = state
                .store
                .put_bytes(METRICS_CF, stat.peer_id.as_bytes(), &value);
            if let Some((_, metrics_value)) = entry.back() {
                for raw in collect_correlations(metrics_value) {
                    let record = CorrelationRecord {
                        metric: raw.metric.clone(),
                        correlation_id: raw.correlation_id.clone(),
                        peer_id: stat.peer_id.clone(),
                        value: raw.value,
                        timestamp: now,
                    };
                    state.record_correlation(&raw.metric, record.clone());
                    if raw.metric == "quic_handshake_fail_total" {
                        state.handle_quic_failure(&record);
                    }
                }
            }
            state.record_tls_warning_samples(&stat.peer_id, &stat.metrics);
            state.record_bridge_anomalies(&stat.peer_id, &stat.metrics, now);
        }
        gauge!(METRIC_CLUSTER_PEER_ACTIVE_TOTAL, map.len() as f64);
    }

    increment_counter!(METRIC_AGGREGATOR_INGEST_TOTAL);
    state.prune();
    state.persist();
    let payload_value = peer_stats_to_value(&payload);
    if let Some(wal) = &state.wal {
        match wal.append(&payload_value) {
            Ok(_) => gauge!(METRIC_AGGREGATOR_REPLICATION_LAG, 0.0),
            Err(err) => warn!(target: "aggregator", error = %err, "failed to append to wal"),
        }
    }
    let blob = json::to_string_value(&payload_value);
    archive_metrics(&blob);

    info!(
        target: "aggregator",
        peers = payload.len(),
        "ingest payload accepted"
    );

    Ok(Response::new(StatusCode::OK)
        .with_header("content-length", "0")
        .close())
}

async fn peer(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let Some(id) = request.param("id") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let data: Vec<(u64, Value)> = state
        .data
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();

    #[cfg(feature = "s3")]
    if let Some(bucket) = S3_BUCKET.as_ref() {
        if let Ok(bytes) = json::to_vec(&data) {
            upload_sync(bucket, bytes);
        }
    }

    Response::new(StatusCode::OK).json(&data)
}

async fn correlations(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let Some(metric) = request.param("metric") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let records = state.correlations_for(metric);
    let value = Value::Array(records.iter().map(CorrelationRecord::to_value).collect());
    json_ok(value)
}

async fn cluster(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let count = state.data.lock().unwrap().len();
    Response::new(StatusCode::OK).json(&count)
}

async fn tls_warning_latest(_request: Request<AppState>) -> Result<Response, HttpError> {
    let mut snapshots = tls_warning_snapshots();
    snapshots.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    let value = Value::Array(snapshots.iter().map(TlsWarningSnapshot::to_value).collect());
    json_ok(value)
}

#[derive(Debug)]
enum ExportError {
    Serialization(String),
    Archive(sys::archive::zip::ZipError),
    Io(io::Error),
    Envelope(EnvelopeError),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Serialization(err) => write!(f, "serialization error: {err}"),
            ExportError::Archive(err) => write!(f, "archive error: {err}"),
            ExportError::Io(err) => write!(f, "io error: {err}"),
            ExportError::Envelope(err) => write!(f, "envelope error: {err}"),
        }
    }
}

impl StdError for ExportError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            ExportError::Serialization(_) => None,
            ExportError::Archive(err) => Some(err),
            ExportError::Io(err) => Some(err),
            ExportError::Envelope(err) => Some(err),
        }
    }
}

impl From<sys::archive::zip::ZipError> for ExportError {
    fn from(value: sys::archive::zip::ZipError) -> Self {
        ExportError::Archive(value)
    }
}

impl From<io::Error> for ExportError {
    fn from(value: io::Error) -> Self {
        ExportError::Io(value)
    }
}

impl From<EnvelopeError> for ExportError {
    fn from(value: EnvelopeError) -> Self {
        ExportError::Envelope(value)
    }
}

struct ExportPayload {
    bytes: Vec<u8>,
    content_type: &'static str,
}

fn build_export_payload(
    map: HashMap<String, VecDeque<(u64, Value)>>,
    tls_snapshots: Vec<TlsWarningSnapshot>,
    tls_status: TlsWarningStatusPayload,
    recipient: Option<String>,
    password: Option<String>,
    bucket: Option<String>,
) -> Result<ExportPayload, ExportError> {
    let mut builder = ZipBuilder::new();
    for (peer_id, deque) in map {
        let json =
            json::to_vec(&deque).map_err(|err| ExportError::Serialization(err.to_string()))?;
        builder.add_file(&format!("{peer_id}.json"), &json)?;
    }

    let tls_latest_value = Value::Array(
        tls_snapshots
            .into_iter()
            .map(|snapshot| snapshot.to_value())
            .collect(),
    );
    let tls_latest = json::to_vec_value(&tls_latest_value);
    builder.add_file("tls_warnings/latest.json", &tls_latest)?;
    let tls_status_bytes = json::to_vec_value(&tls_status.to_value());
    builder.add_file("tls_warnings/status.json", &tls_status_bytes)?;

    let bytes = builder.finish()?;
    let (data, content_type) = match (recipient, password) {
        (Some(recipient), None) => {
            let recipient = x25519::PublicKey::from_str(&recipient)
                .map_err(|err| ExportError::Envelope(err.into()))?;
            let out = envelope::encrypt_for_recipient(&bytes, &recipient)?;
            (out, RECIPIENT_CONTENT_TYPE)
        }
        (None, Some(password)) => {
            let out = envelope::encrypt_with_password(&bytes, password.as_bytes())?;
            (out, PASSWORD_CONTENT_TYPE)
        }
        (None, None) => (bytes, "application/zip"),
        (Some(_), Some(_)) => unreachable!("validated earlier"),
    };

    #[cfg(feature = "s3")]
    if let Some(ref bucket) = bucket {
        upload_sync(bucket, data.clone());
    }
    #[cfg(not(feature = "s3"))]
    let _ = bucket;

    Ok(ExportPayload {
        bytes: data,
        content_type,
    })
}

async fn export_all(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let recipient = request.query_param("recipient").map(|s| s.to_string());
    let password = request.query_param("password").map(|s| s.to_string());

    if recipient.is_some() && password.is_some() {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    }

    let map = {
        let guard = state.data.lock().unwrap();
        if guard.len() > state.max_export_peers {
            return Ok(Response::new(StatusCode::PAYLOAD_TOO_LARGE));
        }
        guard.clone()
    };

    let tls_snapshots = tls_warning_snapshots();
    let tls_status = tls_warning_status_snapshot();

    increment_counter!(METRIC_BULK_EXPORT_TOTAL);

    #[cfg(feature = "s3")]
    let bucket = S3_BUCKET.as_ref().cloned();
    #[cfg(not(feature = "s3"))]
    let bucket: Option<String> = None;

    let handle = spawn_blocking(move || {
        build_export_payload(map, tls_snapshots, tls_status, recipient, password, bucket)
    });
    let payload = handle
        .await
        .map_err(|err| HttpError::Handler(format!("export task join failed: {err}")))?
        .map_err(|err| HttpError::Handler(err.to_string()))?;

    let response = Response::new(StatusCode::OK)
        .with_header("content-type", payload.content_type)
        .with_body(payload.bytes);
    Ok(response)
}

async fn telemetry_post(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let token = state.current_token();
    let authorized = request
        .header("x-auth-token")
        .map(|value| value == token)
        .unwrap_or(false);
    if !authorized {
        return Ok(Response::new(StatusCode::UNAUTHORIZED));
    }

    let payload: Value = request.json()?;
    match telemetry_summary_from_value(&payload) {
        Ok(entry) => {
            increment_counter!(METRIC_TELEMETRY_INGEST_TOTAL);
            state.record_telemetry(entry);
            Ok(Response::new(StatusCode::ACCEPTED))
        }
        Err(err) => {
            increment_counter!(METRIC_TELEMETRY_SCHEMA_ERROR_TOTAL);
            let path = err.path().to_string();
            let message = err.message().to_string();
            warn!(
                target: "aggregator",
                %path,
                %message,
                "telemetry payload rejected by schema guard",
            );
            let body = TelemetryErrorResponse {
                error: message,
                path,
            };
            json_response(StatusCode::BAD_REQUEST, body.to_value())
        }
    }
}

async fn telemetry_index(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let payload = state.telemetry_latest();
    json_ok(telemetry_summary_map_to_value(&payload))
}

async fn telemetry_node(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let Some(node) = request.param("node") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let history = state.telemetry_history(node);
    json_ok(telemetry_history_to_value(&history))
}

async fn wrappers(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let payload = state.wrappers_latest();
    json_ok(wrappers_map_to_value(&payload))
}

async fn metrics(_request: Request<AppState>) -> Result<Response, HttpError> {
    Ok(http_metrics::telemetry_snapshot(
        aggregator_metrics().registry(),
    ))
}

async fn tls_warning_status(_request: Request<AppState>) -> Result<Response, HttpError> {
    let payload = tls_warning_status_snapshot();
    json_ok(payload.to_value())
}

async fn bridge_anomalies(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let events = state.bridge_anomaly_events();
    let value = Value::Array(events.iter().map(BridgeAnomalyEvent::to_value).collect());
    json_ok(value)
}

async fn bridge_remediation(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let actions = state.bridge_remediation_actions();
    let value = Value::Array(
        actions
            .iter()
            .map(BridgeRemediationAction::to_value)
            .collect(),
    );
    json_ok(value)
}

async fn bridge_remediation_dispatches(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let entries = state.bridge_remediation_dispatches();
    let value = Value::Array(
        entries
            .iter()
            .map(BridgeRemediationDispatchRecord::to_value)
            .collect(),
    );
    json_ok(value)
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::new(state)
        .post("/ingest", ingest)
        .get("/peer/:id", peer)
        .get("/correlations/:metric", correlations)
        .get("/cluster", cluster)
        .get("/tls/warnings/latest", tls_warning_latest)
        .get("/tls/warnings/status", tls_warning_status)
        .get("/anomalies/bridge", bridge_anomalies)
        .get("/remediation/bridge", bridge_remediation)
        .get(
            "/remediation/bridge/dispatches",
            bridge_remediation_dispatches,
        )
        .post("/telemetry", telemetry_post)
        .get("/telemetry", telemetry_index)
        .get("/telemetry/:node", telemetry_node)
        .get("/wrappers", wrappers)
        .get("/export/all", export_all)
        .get("/healthz", health)
        .get("/metrics", metrics)
}

async fn health(_request: Request<AppState>) -> Result<Response, HttpError> {
    Ok(Response::new(StatusCode::OK))
}

struct Wal {
    file: Mutex<std::fs::File>,
}

pub async fn run_leader_election(options: Vec<String>, state: AppState) {
    leader::run_with_options(options, state).await;
}

pub async fn run_leader_election_with_config(state: AppState, config: LeaderElectionConfig) {
    leader::run_with_config(state, config).await;
}

impl Wal {
    fn open(path: PathBuf) -> io::Result<Self> {
        use std::fs::OpenOptions;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    fn append(&self, payload: &Value) -> io::Result<()> {
        let mut guard = self.file.lock().unwrap();
        let line = json::to_vec_value(payload);
        guard.write_all(&line)?;
        guard.write_all(b"\n")?;
        guard.flush()
    }
}

fn load_treasury_records(path: &Path) -> io::Result<Vec<TreasuryDisbursement>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                Ok(Vec::new())
            } else {
                let value: Value = json::from_slice(&bytes)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                disbursements_from_json_array(&value)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

fn balance_history_path(disbursement_path: &Path) -> PathBuf {
    let mut path = disbursement_path.to_path_buf();
    path.set_file_name("treasury_balance.json");
    path
}

fn load_treasury_balance_history(path: &Path) -> io::Result<Vec<TreasuryBalanceSnapshot>> {
    let history_path = balance_history_path(path);
    match std::fs::read(&history_path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                Ok(Vec::new())
            } else {
                match json::from_slice(&bytes) {
                    Ok(value) => match balance_history_from_json(&value) {
                        Ok(history) => Ok(history),
                        Err(parse_err) => match parse_legacy_balance_history(&bytes) {
                            Ok(history) => {
                                warn!(
                                    target: "aggregator",
                                    path = %history_path.display(),
                                    error = %parse_err,
                                    "parsed treasury balance history via legacy schema"
                                );
                                Ok(history)
                            }
                            Err(fallback) => Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "decode treasury balance history: {parse_err}; legacy fallback failed: {fallback}"
                                ),
                            )),
                        },
                    },
                    Err(err) => match parse_legacy_balance_history(&bytes) {
                        Ok(history) => {
                            warn!(
                                target: "aggregator",
                                path = %history_path.display(),
                                error = %err,
                                "parsed treasury balance history via legacy schema"
                            );
                            Ok(history)
                        }
                        Err(fallback) => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "decode treasury balance history: {err}; legacy fallback failed: {fallback}"
                            ),
                        )),
                    },
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

fn parse_legacy_balance_history(bytes: &[u8]) -> io::Result<Vec<TreasuryBalanceSnapshot>> {
    let value: Value =
        json::from_slice(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let array = value.as_array().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "treasury balance JSON: expected array",
        )
    })?;
    let mut snapshots = Vec::with_capacity(array.len());
    for entry in array {
        let obj = entry.as_object().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "treasury balance JSON: expected object",
            )
        })?;
        let id = parse_u64_field(obj.get("id"), "id")?;
        let balance_ct = parse_u64_field(obj.get("balance_ct"), "balance_ct")?;
        let delta_ct = parse_i64_field(obj.get("delta_ct"), "delta_ct")?;
        let recorded_at = parse_u64_field(obj.get("recorded_at"), "recorded_at")?;
        let event = parse_event_field(obj.get("event"))?;
        let disbursement_id = match obj.get("disbursement_id") {
            Some(value) => Some(parse_u64_field(Some(value), "disbursement_id")?),
            None => None,
        };
        snapshots.push(TreasuryBalanceSnapshot {
            id,
            balance_ct,
            delta_ct,
            recorded_at,
            event,
            disbursement_id,
        });
    }
    Ok(snapshots)
}

fn parse_u64_field(value: Option<&Value>, field: &str) -> io::Result<u64> {
    match value {
        Some(Value::Number(num)) => num.as_u64().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{field} is not a u64"))
        }),
        Some(Value::String(raw)) => raw.parse::<u64>().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{field} string parse error: {err}"),
            )
        }),
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{field} has unexpected type {other:?}"),
        )),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing field {field}"),
        )),
    }
}

fn parse_i64_field(value: Option<&Value>, field: &str) -> io::Result<i64> {
    match value {
        Some(Value::Number(num)) => num.as_i64().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{field} is not an i64"))
        }),
        Some(Value::String(raw)) => raw.parse::<i64>().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{field} string parse error: {err}"),
            )
        }),
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{field} has unexpected type {other:?}"),
        )),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing field {field}"),
        )),
    }
}

fn parse_event_field(value: Option<&Value>) -> io::Result<TreasuryBalanceEventKind> {
    let Some(Value::String(raw)) = value else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "treasury balance JSON: missing event",
        ));
    };
    match raw {
        s if s.eq_ignore_ascii_case("accrual") => Ok(TreasuryBalanceEventKind::Accrual),
        s if s.eq_ignore_ascii_case("queued") => Ok(TreasuryBalanceEventKind::Queued),
        s if s.eq_ignore_ascii_case("executed") => Ok(TreasuryBalanceEventKind::Executed),
        s if s.eq_ignore_ascii_case("cancelled") => Ok(TreasuryBalanceEventKind::Cancelled),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("treasury balance JSON: unknown event {other}"),
        )),
    }
}

#[derive(Default)]
struct TreasurySummary {
    scheduled_count: u64,
    scheduled_amount: u64,
    executed_count: u64,
    executed_amount: u64,
    cancelled_count: u64,
    cancelled_amount: u64,
    latest_timestamp: Option<u64>,
    oldest_scheduled_created: Option<u64>,
    next_epoch: Option<u64>,
}

impl TreasurySummary {
    fn from_records(records: &[TreasuryDisbursement]) -> Self {
        let mut summary = TreasurySummary::default();
        for record in records {
            match &record.status {
                DisbursementStatus::Scheduled => {
                    summary.scheduled_count = summary.scheduled_count.saturating_add(1);
                    summary.scheduled_amount =
                        summary.scheduled_amount.saturating_add(record.amount_ct);
                    summary.update_latest(record.created_at);
                    summary.oldest_scheduled_created = match summary.oldest_scheduled_created {
                        Some(oldest) => Some(oldest.min(record.created_at)),
                        None => Some(record.created_at),
                    };
                    summary.next_epoch = match summary.next_epoch {
                        Some(epoch) => Some(epoch.min(record.scheduled_epoch)),
                        None => Some(record.scheduled_epoch),
                    };
                }
                DisbursementStatus::Executed { executed_at, .. } => {
                    summary.executed_count = summary.executed_count.saturating_add(1);
                    summary.executed_amount =
                        summary.executed_amount.saturating_add(record.amount_ct);
                    summary.update_latest(*executed_at);
                }
                DisbursementStatus::Cancelled { cancelled_at, .. } => {
                    summary.cancelled_count = summary.cancelled_count.saturating_add(1);
                    summary.cancelled_amount =
                        summary.cancelled_amount.saturating_add(record.amount_ct);
                    summary.update_latest(*cancelled_at);
                }
            }
        }
        summary
    }

    fn update_latest(&mut self, ts: u64) {
        self.latest_timestamp = Some(match self.latest_timestamp {
            Some(prev) => prev.max(ts),
            None => ts,
        });
    }

    fn metrics_for_status(&self, status: &str) -> (u64, u64) {
        match status {
            "scheduled" => (self.scheduled_count, self.scheduled_amount),
            "executed" => (self.executed_count, self.executed_amount),
            "cancelled" => (self.cancelled_count, self.cancelled_amount),
            _ => (0, 0),
        }
    }

    fn snapshot_age(&self, now: u64) -> u64 {
        self.latest_timestamp
            .map(|ts| now.saturating_sub(ts))
            .unwrap_or(0)
    }

    fn scheduled_oldest_age(&self, now: u64) -> u64 {
        self.oldest_scheduled_created
            .map(|ts| now.saturating_sub(ts))
            .unwrap_or(0)
    }

    fn next_epoch_value(&self) -> u64 {
        self.next_epoch.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::encryption::{
        envelope::{self, PASSWORD_CONTENT_TYPE, RECIPIENT_CONTENT_TYPE},
        x25519::SecretKey,
    };
    use crypto_suite::hashing::blake3;
    use foundation_telemetry::WrapperMetricEntry;
    use http_env::server_tls_from_env;
    use httpd::{Method, StatusCode};
    use rand::rngs::OsRng;
    use std::collections::HashMap;
    use std::future::Future;
    use std::time::{SystemTime, UNIX_EPOCH};
    use sys::archive::zip::ZipReader;
    use sys::tempfile;

    fn run_async<T>(future: impl Future<Output = T>) -> T {
        runtime::block_on(future)
    }

    fn parse_json(input: &str) -> Value {
        json::value_from_str(input).expect("valid test json")
    }

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("monotonic clock")
            .as_nanos()
    }

    #[test]
    fn dedupes_by_peer() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("token".into(), dir.path().join("m.json"), 60);
            let app = router(state.clone());
            let payload = parse_json(
                r#"[
                {"peer_id":"a","metrics":{"r":1}},
                {"peer_id":"a","metrics":{"r":2}}
            ]"#,
            );
            let ingest = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(ingest).await.unwrap();

            let resp = app
                .handle(app.request_builder().path("/peer/a").build())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let vals: Vec<(u64, Value)> = json::from_slice(resp.body()).unwrap();
            assert_eq!(vals.len(), 1);
            let metrics = vals[0]
                .1
                .as_object()
                .and_then(|map| map.get("r"))
                .and_then(|value| value.as_i64())
                .unwrap();
            assert_eq!(metrics, 3);
        });
    }

    #[test]
    fn persists_and_prunes() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("m.json");
            {
                let state = AppState::new("t".into(), &path, 1);
                let app = router(state.clone());
                let payload = parse_json(r#"[{"peer_id":"p","metrics":{"v":1}}]"#);
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "t")
                    .json(&payload)
                    .unwrap()
                    .build();
                let _ = app.handle(req).await.unwrap();
            }
            // Reload and ensure data persisted
            let state = AppState::new("t".into(), &path, 1);
            {
                let map = state.data.lock().unwrap();
                assert!(map.contains_key("p"));
            }
            // Insert artificially old data and prune
            {
                let mut map = state.data.lock().unwrap();
                if let Some(deque) = map.get_mut("p") {
                    if let Some(entry) = deque.front_mut() {
                        entry.0 = 0; // timestamp far in past
                    }
                }
            }
            state.prune();
            let map = state.data.lock().unwrap();
            assert!(map.get("p").map(|d| d.is_empty()).unwrap_or(true));
        });
    }

    #[test]
    fn tls_env_warnings_increment_metric() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        let metrics = aggregator_metrics();
        let prefix = format!("TB_TEST_TLS_{}", unique_suffix());
        metrics
            .tls_env_warning_total
            .remove_label_values(&[prefix.as_str(), "missing_identity_component"]);
        metrics.tls_env_warning_events_total.remove_label_values(&[
            prefix.as_str(),
            "missing_identity_component",
            "diagnostics",
        ]);
        let _ = metrics
            .tls_env_warning_last_seen
            .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
            .map(|handle| handle.set(0.0));

        let cert_var = format!("{prefix}_CERT");
        let key_var = format!("{prefix}_KEY");
        let client_ca_var = format!("{prefix}_CLIENT_CA");
        let client_ca_optional_var = format!("{prefix}_CLIENT_CA_OPTIONAL");

        std::env::set_var(&cert_var, "/tmp/test-aggregator-cert.pem");
        std::env::remove_var(&key_var);
        std::env::remove_var(&client_ca_var);
        std::env::remove_var(&client_ca_optional_var);

        let _ = server_tls_from_env(&prefix, None);

        let counter = metrics
            .tls_env_warning_total
            .get_metric_with_label_values(&[prefix.as_str(), "missing_identity_component"])
            .expect("registered label set");
        assert_eq!(counter.get(), 1);
        let events = metrics
            .tls_env_warning_events_total
            .get_metric_with_label_values(&[
                prefix.as_str(),
                "missing_identity_component",
                "diagnostics",
            ])
            .expect("registered origin label set");
        assert_eq!(events.get(), 1);
        let gauge = metrics
            .tls_env_warning_last_seen
            .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
            .expect("gauge handle");
        assert!(gauge.get() > 0.0);

        let snapshot = tls_warning_snapshot(&prefix, "missing_identity_component")
            .expect("snapshot recorded for missing_key");
        assert_eq!(snapshot.total, 1);
        assert_eq!(snapshot.last_delta, 1);
        assert_eq!(snapshot.origin, WarningOrigin::Diagnostics);
        assert_eq!(snapshot.peer_id, None);
        let expected_detail = format!(
            "identity requires both {cert} and {key}; missing {key}",
            cert = cert_var,
            key = key_var
        );
        assert_eq!(snapshot.detail.as_deref(), Some(expected_detail.as_str()));
        assert_eq!(snapshot.variables, vec![key_var.clone()]);
        assert!(snapshot.last_seen > 0);

        std::env::remove_var(cert_var);
        std::env::remove_var(key_var);
        std::env::remove_var(client_ca_var);
        std::env::remove_var(client_ca_optional_var);
    }

    #[test]
    fn tls_env_warning_ingest_updates_counter() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        let metrics = aggregator_metrics();
        metrics
            .tls_env_warning_total
            .remove_label_values(&["TB_NODE_TLS", "missing_anchor"]);
        metrics.tls_env_warning_events_total.remove_label_values(&[
            "TB_NODE_TLS",
            "missing_anchor",
            "peer_ingest",
        ]);

        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("ingest.json"), 60);
            let app = router(state.clone());

            let payload = parse_json(
                r#"[
                    {
                        "peer_id": "node-a",
                        "metrics": {
                            "tls_env_warning_total": [
                                {"labels": {"prefix": "TB_NODE_TLS", "code": "missing_anchor"}, "value": 2.0}
                            ]
                        }
                    }
                ]"#,
            );
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "t")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(req).await.unwrap();

            let payload = parse_json(
                r#"[
                    {
                        "peer_id": "node-a",
                        "metrics": {
                            "tls_env_warning_total": [
                                {"labels": {"prefix": "TB_NODE_TLS", "code": "missing_anchor"}, "value": 3.0}
                            ]
                        }
                    }
                ]"#,
            );
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "t")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(req).await.unwrap();
        });

        let counter = metrics
            .tls_env_warning_total
            .get_metric_with_label_values(&["TB_NODE_TLS", "missing_anchor"])
            .expect("registered label set");
        assert_eq!(counter.get(), 3);
        let events = metrics
            .tls_env_warning_events_total
            .get_metric_with_label_values(&["TB_NODE_TLS", "missing_anchor", "peer_ingest"])
            .expect("registered origin label set");
        assert_eq!(events.get(), 3);
        let gauge = metrics
            .tls_env_warning_last_seen
            .ensure_handle_for_label_values(&["TB_NODE_TLS", "missing_anchor"])
            .expect("gauge handle");
        assert!(gauge.get() > 0.0);
        let snapshot = tls_warning_snapshot("TB_NODE_TLS", "missing_anchor")
            .expect("snapshot recorded for missing_anchor");
        assert_eq!(snapshot.total, 3);
        assert_eq!(snapshot.last_delta, 1);
        assert_eq!(snapshot.origin, WarningOrigin::PeerIngest);
        assert_eq!(snapshot.peer_id.as_deref(), Some("node-a"));
        assert!(snapshot.detail.is_none());
        assert!(snapshot.variables.is_empty());
        assert!(snapshot.last_seen > 0);
    }

    #[test]
    fn tls_env_warning_gauge_rehydrates_last_seen() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        let metrics = aggregator_metrics();
        metrics
            .tls_env_warning_total
            .remove_label_values(&["TB_NODE_TLS", "missing_anchor"]);
        metrics
            .tls_env_warning_last_seen
            .remove_label_values(&["TB_NODE_TLS", "missing_anchor"]);

        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("rehydrate.json"), 60);
            let app = router(state.clone());

            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("monotonic clock")
                .as_secs();
            let payload = parse_json(&format!(
                r#"[
                    {{
                        "peer_id": "node-b",
                        "metrics": {{
                            "tls_env_warning_last_seen_seconds": [
                                {{"labels": {{"prefix": "TB_NODE_TLS", "code": "missing_anchor"}}, "value": {}}}
                            ]
                        }}
                    }}
                ]"#,
                now_secs
            ));

            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "t")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(req).await.unwrap();
        });

        let snapshot = tls_warning_snapshot("TB_NODE_TLS", "missing_anchor")
            .expect("snapshot recorded for gauge");
        assert_eq!(snapshot.total, 0);
        assert_eq!(snapshot.last_delta, 0);
        assert_eq!(snapshot.origin, WarningOrigin::PeerIngest);
        assert_eq!(snapshot.peer_id.as_deref(), Some("node-b"));
        assert!(snapshot.detail.is_none());
        assert!(snapshot.variables.is_empty());
        assert!(snapshot.last_seen > 0);

        let gauge = metrics
            .tls_env_warning_last_seen
            .ensure_handle_for_label_values(&["TB_NODE_TLS", "missing_anchor"])
            .expect("gauge handle");
        assert_eq!(gauge.get().round() as u64, snapshot.last_seen);
    }

    #[test]
    fn tls_warning_latest_endpoint_exposes_snapshots() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        let metrics = aggregator_metrics();
        let prefix = format!("TB_FLEET_TLS_{}", unique_suffix());
        let code = "missing_identity_component";
        let _ = metrics
            .tls_env_warning_total
            .remove_label_values(&[prefix.as_str(), code]);

        let cert_var = format!("{prefix}_CERT");
        let key_var = format!("{prefix}_KEY");
        std::env::set_var(&cert_var, "/tmp/test-fleet-cert.pem");
        std::env::remove_var(&key_var);
        std::env::remove_var(format!("{prefix}_CLIENT_CA"));
        std::env::remove_var(format!("{prefix}_CLIENT_CA_OPTIONAL"));

        let _ = server_tls_from_env(&prefix, None);

        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("token".into(), dir.path().join("state.db"), 60);
            let app = router(state);
            let resp = app
                .handle(app.request_builder().path("/tls/warnings/latest").build())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let value: Value = json::from_slice(resp.body()).unwrap();
            let array = value.as_array().expect("array payload");
            assert!(!array.is_empty());
            let entry = array
                .iter()
                .find(|item| {
                    item.get("prefix").and_then(Value::as_str) == Some(prefix.as_str())
                        && item.get("code").and_then(Value::as_str) == Some(code)
                })
                .expect("fleet tls snapshot");
            assert_eq!(
                entry.get("prefix").and_then(Value::as_str),
                Some(prefix.as_str())
            );
            assert_eq!(
                entry.get("origin").and_then(Value::as_str),
                Some("diagnostics")
            );
            let detail = entry
                .get("detail")
                .and_then(Value::as_str)
                .expect("detail string");
            assert!(detail.contains(&key_var));
            let vars = entry
                .get("variables")
                .and_then(Value::as_array)
                .expect("variables array");
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0], foundation_serialization::json!(key_var.as_str()));
        });

        let _ = metrics
            .tls_env_warning_total
            .remove_label_values(&[prefix.as_str(), code]);
        std::env::remove_var(cert_var);
        std::env::remove_var(key_var);
    }

    #[test]
    fn tls_warning_snapshots_prune_stale_entries() {
        reset_tls_warning_snapshots();
        {
            let mut guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
            let mut old = TlsWarningSnapshot::new("TB_OLD", "expired");
            old.last_seen = 1;
            guard.insert(("TB_OLD".into(), "expired".into()), old);

            let mut fresh = TlsWarningSnapshot::new("TB_FRESH", "active");
            fresh.last_seen = TLS_WARNING_SNAPSHOT_RETENTION_SECS + 100;
            guard.insert(("TB_FRESH".into(), "active".into()), fresh);
        }

        prune_tls_warning_snapshots_for_test(TLS_WARNING_SNAPSHOT_RETENTION_SECS + 200);
        let snapshots = tls_warning_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].prefix, "TB_FRESH");
        assert_eq!(snapshots[0].code, "active");
    }

    #[test]
    fn tls_warning_retention_override_applies() {
        reset_tls_warning_snapshots();
        let dir = tempfile::tempdir().unwrap();
        let _state = AppState::new_with_opts(
            "token".into(),
            None,
            dir.path().join("override.db"),
            60,
            None,
            Some(10),
            None,
        );

        {
            let mut guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
            let mut stale = TlsWarningSnapshot::new("TB_OVERRIDE", "stale");
            stale.last_seen = 1;
            guard.insert(("TB_OVERRIDE".into(), "stale".into()), stale);

            let mut fresh = TlsWarningSnapshot::new("TB_OVERRIDE", "fresh");
            fresh.last_seen = 15;
            guard.insert(("TB_OVERRIDE".into(), "fresh".into()), fresh);
        }

        prune_tls_warning_snapshots_for_test(20);
        let snapshots = tls_warning_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].prefix, "TB_OVERRIDE");
        assert_eq!(snapshots[0].code, "fresh");
    }

    #[test]
    fn tls_warning_status_reports_counts_and_retention() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        let metrics = aggregator_metrics();
        let _ = metrics
            .tls_env_warning_total
            .remove_label_values(&["TB_STATUS_TLS", "missing_anchor"]);
        let dir = tempfile::tempdir().unwrap();
        let _state = AppState::new_with_opts(
            "token".into(),
            None,
            dir.path().join("status.db"),
            60,
            None,
            Some(15),
            None,
        );

        record_tls_env_warning_event(
            "TB_STATUS_TLS",
            "missing_anchor",
            2,
            TlsWarningMetadata::peer("node-a"),
        );
        {
            let mut guard = TLS_WARNING_SNAPSHOTS.lock().unwrap();
            if let Some(entry) = guard.get_mut(&("TB_STATUS_TLS".into(), "missing_anchor".into())) {
                entry.last_seen = 1;
            }
        }

        let payload = tls_warning_status_snapshot();
        assert_eq!(payload.retention_seconds, 15);
        assert_eq!(payload.active_snapshots, 1);
        assert_eq!(payload.stale_snapshots, 1);
        assert_eq!(payload.least_recent_last_seen, Some(1));
        assert_eq!(payload.most_recent_last_seen, Some(1));
        assert_eq!(metrics.tls_env_warning_retention_seconds.get(), 15.0);
        assert_eq!(metrics.tls_env_warning_active_snapshots.get(), 1.0);
        assert_eq!(metrics.tls_env_warning_stale_snapshots.get(), 1.0);
        assert_eq!(
            metrics.tls_env_warning_most_recent_last_seen.get().round() as u64,
            1
        );
        assert_eq!(
            metrics.tls_env_warning_least_recent_last_seen.get().round() as u64,
            1
        );
    }

    #[test]
    fn tls_warning_status_endpoint_exposes_payload() {
        install_tls_env_warning_forwarder();
        reset_tls_warning_snapshots();
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new_with_opts(
                "token".into(),
                None,
                dir.path().join("status_endpoint.db"),
                60,
                None,
                Some(42),
                None,
            );
            let app = router(state.clone());

            record_tls_env_warning_event(
                "TB_STATUS_TLS",
                "missing_anchor",
                1,
                TlsWarningMetadata::peer("node-a"),
            );

            let resp = app
                .handle(app.request_builder().path("/tls/warnings/status").build())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let value: Value = json::from_slice(resp.body()).unwrap();
            assert_eq!(
                value
                    .get("retention_seconds")
                    .and_then(Value::as_u64)
                    .expect("retention seconds"),
                42
            );
            assert_eq!(
                value
                    .get("active_snapshots")
                    .and_then(Value::as_u64)
                    .expect("active snapshots"),
                1
            );
            assert_eq!(
                value
                    .get("stale_snapshots")
                    .and_then(Value::as_u64)
                    .expect("stale snapshots"),
                0
            );
        });
    }

    #[test]
    fn tls_env_warning_ingest_handles_nested_samples() {
        install_tls_env_warning_forwarder();
        let metrics = aggregator_metrics();
        for labels in [
            ["TB_NODE_TLS", "missing_anchor"],
            ["TB_NODE_TLS", "mismatched_chain"],
            ["TB_GATEWAY_TLS", "expired_certificate"],
        ] {
            let _ = metrics.tls_env_warning_total.remove_label_values(&labels);
        }

        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("nested.json"), 60);
            let app = router(state.clone());

            let payload = parse_json(
                r#"
                [
                    {
                        "peer_id": "node-a",
                        "metrics": {
                            "tls_env_warning_total": [
                                {
                                    "labels": {"prefix": "TB_NODE_TLS", "code": "missing_anchor"},
                                    "value": 2.0,
                                    "samples": [
                                        {"prefix": "TB_NODE_TLS", "code": "missing_anchor", "counter": 2.0},
                                        {"labels": {"prefix": "TB_NODE_TLS", "code": "mismatched_chain"}, "value": 1.0}
                                    ],
                                    "children": [
                                        {"labels": {"prefix": "TB_NODE_TLS", "code": "missing_anchor"}, "value": 2.0}
                                    ]
                                }
                            ]
                        }
                    },
                    {
                        "peer_id": "node-b",
                        "metrics": {
                            "tls_env_warning_total": {
                                "samples": [
                                    {"labels": {"prefix": "TB_GATEWAY_TLS", "code": "expired_certificate"}, "value": 4.0}
                                ]
                            }
                        }
                    }
                ]
                "#,
            );
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "t")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(req).await.unwrap();

            let payload = parse_json(
                r#"
                [
                    {
                        "peer_id": "node-a",
                        "metrics": {
                            "tls_env_warning_total": [
                                {
                                    "prefix": "TB_NODE_TLS",
                                    "code": "missing_anchor",
                                    "value": 5.0,
                                    "samples": [
                                        {"labels": {"prefix": "TB_NODE_TLS", "code": "missing_anchor"}, "counter": 5.0},
                                        {"labels": {"prefix": "TB_NODE_TLS", "code": "mismatched_chain"}, "counter": 2.0}
                                    ]
                                }
                            ]
                        }
                    },
                    {
                        "peer_id": "node-b",
                        "metrics": {
                            "tls_env_warning_total": [
                                {
                                    "labels": {"prefix": "TB_GATEWAY_TLS", "code": "expired_certificate"},
                                    "counter": 7.0
                                }
                            ]
                        }
                    }
                ]
                "#,
            );
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "t")
                .json(&payload)
                .unwrap()
                .build();
            let _ = app.handle(req).await.unwrap();
        });

        let missing_anchor = metrics
            .tls_env_warning_total
            .get_metric_with_label_values(&["TB_NODE_TLS", "missing_anchor"])
            .expect("missing_anchor label set");
        assert_eq!(missing_anchor.get(), 5);

        let mismatched_chain = metrics
            .tls_env_warning_total
            .get_metric_with_label_values(&["TB_NODE_TLS", "mismatched_chain"])
            .expect("mismatched_chain label set");
        assert_eq!(mismatched_chain.get(), 2);

        let expired_certificate = metrics
            .tls_env_warning_total
            .get_metric_with_label_values(&["TB_GATEWAY_TLS", "expired_certificate"])
            .expect("expired_certificate label set");
        assert_eq!(expired_certificate.get(), 7);

        for labels in [
            ["TB_NODE_TLS", "missing_anchor"],
            ["TB_NODE_TLS", "mismatched_chain"],
            ["TB_GATEWAY_TLS", "expired_certificate"],
        ] {
            let _ = metrics.tls_env_warning_total.remove_label_values(&labels);
        }
    }

    #[test]
    fn export_all_zips_and_checksums() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = parse_json(
                    r#"[
                        {"peer_id":"p1","metrics":{"v":1}},
                        {"peer_id":"p2","metrics":{"v":2}}
                    ]"#,
                );
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "t")
                    .json(&payload)
                    .unwrap()
                    .build();
                let _ = app.handle(req).await.unwrap();
            }
            let app = router(state);
            let resp = app
                .handle(app.request_builder().path("/export/all").build())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = resp.body().to_vec();
            let hash = blake3::hash(&body_bytes);
            assert_ne!(hash.as_bytes(), &[0u8; 32]);
            let archive = ZipReader::from_bytes(&body_bytes).unwrap();
            assert_eq!(archive.len(), 4);
            let file = archive.file("p1.json").unwrap();
            let v: Vec<(u64, Value)> = json::from_slice(file).unwrap();
            let metric = v[0]
                .1
                .as_object()
                .and_then(|map| map.get("v"))
                .and_then(|value| value.as_i64())
                .unwrap();
            assert_eq!(metric, 1);
            let latest: Vec<Value> =
                json::from_slice(archive.file("tls_warnings/latest.json").unwrap()).unwrap();
            assert!(latest.is_empty());
            let status: Value =
                json::from_slice(archive.file("tls_warnings/status.json").unwrap()).unwrap();
            assert!(status.get("retention_seconds").is_some());
        });
    }

    #[test]
    fn export_all_encrypts() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = parse_json(r#"[{"peer_id":"p1","metrics":{"v":1}}]"#);
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "t")
                    .json(&payload)
                    .unwrap()
                    .build();
                let _ = app.handle(req).await.unwrap();
            }
            let mut rng = OsRng::default();
            let secret = SecretKey::generate(&mut rng);
            let recipient = secret.public_key().to_string();
            let app = router(state);
            let resp = app
                .handle(
                    app.request_builder()
                        .path(format!("/export/all?recipient={recipient}"))
                        .build(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(resp.header("content-type"), Some(RECIPIENT_CONTENT_TYPE));
            let body_bytes = resp.body().to_vec();
            let plain = envelope::decrypt_with_secret(&body_bytes, &secret).unwrap();
            let archive = ZipReader::from_bytes(&plain).unwrap();
            assert_eq!(archive.len(), 3);
            let file = archive.file("p1.json").unwrap();
            let v: Vec<(u64, Value)> = json::from_slice(file).unwrap();
            let metric = v[0]
                .1
                .as_object()
                .and_then(|map| map.get("v"))
                .and_then(|value| value.as_i64())
                .unwrap();
            assert_eq!(metric, 1);
        });
    }

    #[test]
    fn export_all_password_encrypts() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = parse_json(r#"[{"peer_id":"p1","metrics":{"v":1}}]"#);
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "t")
                    .json(&payload)
                    .unwrap()
                    .build();
                let _ = app.handle(req).await.unwrap();
            }
            let app = router(state);
            let resp = app
                .handle(
                    app.request_builder()
                        .path("/export/all?password=secret")
                        .build(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(resp.header("content-type"), Some(PASSWORD_CONTENT_TYPE));
            let body_bytes = resp.body().to_vec();
            let plain = envelope::decrypt_with_password(&body_bytes, b"secret").unwrap();
            let archive = ZipReader::from_bytes(&plain).unwrap();
            assert_eq!(archive.len(), 3);
            let file = archive.file("p1.json").unwrap();
            let v: Vec<(u64, Value)> = json::from_slice(file).unwrap();
            let metric = v[0]
                .1
                .as_object()
                .and_then(|map| map.get("v"))
                .and_then(|value| value.as_i64())
                .unwrap();
            assert_eq!(metric, 1);
        });
    }

    #[test]
    fn wrappers_endpoint_returns_latest_metrics() {
        run_async(async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            state.record_telemetry(TelemetrySummary {
                node_id: "node-a".into(),
                seq: 1,
                timestamp: 123,
                sample_rate_ppm: 1,
                compaction_secs: 30,
                memory: HashMap::new(),
                wrappers: WrapperSummaryEntry {
                    metrics: vec![WrapperMetricEntry {
                        metric: "codec_serialize_fail_total".into(),
                        labels: HashMap::from([
                            ("codec".into(), "json".into()),
                            ("profile".into(), "none".into()),
                            ("version".into(), "1.2.3".into()),
                        ]),
                        value: 2.0,
                    }],
                },
            });

            let app = router(state);
            let resp = app
                .handle(app.request_builder().path("/wrappers").build())
                .await
                .unwrap();

            assert_eq!(resp.status(), StatusCode::OK);
            let parsed: Value = json::from_slice(resp.body()).unwrap();
            let entry = parsed
                .as_object()
                .and_then(|map| map.get("node-a"))
                .and_then(Value::as_object)
                .expect("wrapper entry");
            let metrics = entry
                .get("metrics")
                .and_then(Value::as_array)
                .expect("metrics array");
            assert_eq!(metrics.len(), 1);
            let metric = metrics[0].as_object().expect("metric object");
            assert_eq!(
                metric.get("metric").and_then(Value::as_str),
                Some("codec_serialize_fail_total")
            );
            let labels = metric
                .get("labels")
                .and_then(Value::as_object)
                .expect("labels object");
            assert_eq!(labels.get("codec").and_then(Value::as_str), Some("json"));
        });
    }
}
