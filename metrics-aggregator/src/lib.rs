use concurrency::Lazy;
use diagnostics::{
    internal::{install_tls_env_warning_subscriber, SubscriberGuard as LoggingSubscriberGuard},
    tracing::{info, warn},
};
use foundation_metrics::{gauge, increment_counter, Recorder, RecorderInstallError};
use governance::{
    DisbursementStatus, GovStore, TreasuryBalanceEventKind, TreasuryBalanceSnapshot,
    TreasuryDisbursement,
};
use http_env::{http_client as env_http_client, register_tls_warning_sink, TlsEnvWarningSinkGuard};
use httpd::metrics as http_metrics;
use httpd::uri::form_urlencoded;
use httpd::{HttpClient, HttpError, Method, Request, Response, Router, StatusCode};
use runtime::telemetry::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, IntGaugeVec, Opts, Registry,
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

use foundation_serialization::json::{Map, Number, Value};
use foundation_serialization::{json, Deserialize, Serialize};
use foundation_telemetry::{TelemetrySummary, WrapperSummaryEntry};
use std::collections::{btree_map::Entry, BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
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
const COUNTER_EPSILON: f64 = 1e-6;
const TLS_WARNING_SNAPSHOT_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;
static TLS_WARNING_RETENTION_SECS: AtomicU64 = AtomicU64::new(TLS_WARNING_SNAPSHOT_RETENTION_SECS);

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

#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: Value,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TelemetryErrorResponse {
    error: String,
    path: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct CorrelationRecord {
    pub metric: String,
    pub correlation_id: String,
    pub peer_id: String,
    pub value: Option<f64>,
    pub timestamp: u64,
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
            leader_flag: Arc::new(AtomicBool::new(false)),
            leader_id: Arc::new(RwLock::new(None)),
            leader_fencing: Arc::new(AtomicU64::new(0)),
            treasury_source,
        };
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
            let mut ticker = runtime::interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                state.prune();
                state.refresh_treasury_metrics();
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
}

#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(crate = "foundation_serialization::serde")]
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
    }
});

fn aggregator_metrics() -> &'static AggregatorMetrics {
    Lazy::force(&METRICS)
}

static TLS_WARNING_SNAPSHOTS: Lazy<Mutex<HashMap<(String, String), TlsWarningSnapshot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct TlsWarningStatusPayload {
    retention_seconds: u64,
    active_snapshots: usize,
    stale_snapshots: usize,
    most_recent_last_seen: Option<u64>,
    least_recent_last_seen: Option<u64>,
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

    let payload: Vec<PeerStat> = request.json()?;
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
        }
        gauge!(METRIC_CLUSTER_PEER_ACTIVE_TOTAL, map.len() as f64);
    }

    increment_counter!(METRIC_AGGREGATOR_INGEST_TOTAL);
    state.prune();
    state.persist();
    if let Some(wal) = &state.wal {
        match wal.append(&payload) {
            Ok(_) => gauge!(METRIC_AGGREGATOR_REPLICATION_LAG, 0.0),
            Err(err) => warn!(target: "aggregator", error = %err, "failed to append to wal"),
        }
    }
    if let Ok(blob) = json::to_string(&payload) {
        archive_metrics(&blob);
    }

    Ok(Response::new(StatusCode::OK))
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
    Response::new(StatusCode::OK).json(&records)
}

async fn cluster(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let count = state.data.lock().unwrap().len();
    Response::new(StatusCode::OK).json(&count)
}

async fn tls_warning_latest(_request: Request<AppState>) -> Result<Response, HttpError> {
    let mut snapshots = tls_warning_snapshots();
    snapshots.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    Response::new(StatusCode::OK).json(&snapshots)
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

    let tls_latest =
        json::to_vec(&tls_snapshots).map_err(|err| ExportError::Serialization(err.to_string()))?;
    builder.add_file("tls_warnings/latest.json", &tls_latest)?;
    let tls_status_bytes =
        json::to_vec(&tls_status).map_err(|err| ExportError::Serialization(err.to_string()))?;
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
    match TelemetrySummary::from_value(payload) {
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
            let response = Response::new(StatusCode::BAD_REQUEST).json(&body)?;
            Ok(response)
        }
    }
}

async fn telemetry_index(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let payload = state.telemetry_latest();
    Response::new(StatusCode::OK).json(&payload)
}

async fn telemetry_node(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let Some(node) = request.param("node") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let history = state.telemetry_history(node);
    Response::new(StatusCode::OK).json(&history)
}

async fn wrappers(request: Request<AppState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let payload = state.wrappers_latest();
    Response::new(StatusCode::OK).json(&payload)
}

async fn metrics(_request: Request<AppState>) -> Result<Response, HttpError> {
    Ok(http_metrics::telemetry_snapshot(
        aggregator_metrics().registry(),
    ))
}

async fn tls_warning_status(_request: Request<AppState>) -> Result<Response, HttpError> {
    let payload = tls_warning_status_snapshot();
    Response::new(StatusCode::OK).json(&payload)
}

pub fn router(state: AppState) -> Router<AppState> {
    Router::new(state)
        .post("/ingest", ingest)
        .get("/peer/:id", peer)
        .get("/correlations/:metric", correlations)
        .get("/cluster", cluster)
        .get("/tls/warnings/latest", tls_warning_latest)
        .get("/tls/warnings/status", tls_warning_status)
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

    fn append(&self, stats: &[PeerStat]) -> io::Result<()> {
        let mut guard = self.file.lock().unwrap();
        let line = json::to_vec(&stats.to_vec())
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
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
                json::from_slice(&bytes)
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
                    Ok(history) => Ok(history),
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
            let parsed: HashMap<String, WrapperSummaryEntry> =
                json::from_slice(resp.body()).unwrap();
            let entry = parsed.get("node-a").expect("wrapper entry");
            assert_eq!(entry.metrics.len(), 1);
            assert_eq!(entry.metrics[0].metric, "codec_serialize_fail_total");
            assert_eq!(
                entry.metrics[0].labels.get("codec").map(String::as_str),
                Some("json")
            );
        });
    }
}
