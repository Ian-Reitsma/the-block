use concurrency::Lazy;
use diagnostics::tracing::{info, warn};
use http_env::http_client as env_http_client;
use httpd::metrics as http_metrics;
use httpd::uri::form_urlencoded;
use httpd::{HttpClient, HttpError, Method, Request, Response, Router, StatusCode};
use runtime::telemetry::{Counter, Gauge, Registry};
use runtime::{spawn, spawn_blocking};
use std::error::Error as StdError;
use std::fmt;
use std::str::FromStr;
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
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use storage_engine::{inhouse_engine::InhouseEngine, KeyValue, KeyValueIterator};

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

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: Value,
}

#[derive(Serialize)]
struct TelemetryErrorResponse {
    error: String,
    path: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
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
    leader_flag: Arc<AtomicBool>,
    leader_id: Arc<RwLock<Option<String>>>,
    leader_fencing: Arc<AtomicU64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderSnapshot {
    pub is_leader: bool,
    pub leader_id: Option<String>,
    pub fencing_token: u64,
}

impl AppState {
    pub fn new(token: String, path: impl AsRef<Path>, retention_secs: u64) -> Self {
        Self::new_with_opts(token, None, path, retention_secs, None)
    }

    pub fn new_with_opts(
        token: String,
        token_path: Option<PathBuf>,
        path: impl AsRef<Path>,
        retention_secs: u64,
        wal: Option<PathBuf>,
    ) -> Self {
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
            leader_flag: Arc::new(AtomicBool::new(false)),
            leader_id: Arc::new(RwLock::new(None)),
            leader_fencing: Arc::new(AtomicU64::new(0)),
        };
        state.prune();
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
            aggregator_metrics().retention_pruned_total.inc_by(removed);
            let _ = self.store.flush();
        }
        removed
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
            let mut ticker = runtime::interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                state.prune();
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
    retention_pruned_total: Counter,
    telemetry_ingest_total: Counter,
    telemetry_schema_error_total: Counter,
}

impl AggregatorMetrics {
    fn registry(&self) -> &Registry {
        &self.registry
    }
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
    AggregatorMetrics {
        registry,
        ingest_total,
        bulk_export_total,
        active_peers,
        replication_lag,
        retention_pruned_total,
        telemetry_ingest_total,
        telemetry_schema_error_total,
    }
});

fn aggregator_metrics() -> &'static AggregatorMetrics {
    Lazy::force(&METRICS)
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
        }
        aggregator_metrics().active_peers.set(map.len() as f64);
    }

    aggregator_metrics().ingest_total.inc();
    state.prune();
    state.persist();
    if let Some(wal) = &state.wal {
        match wal.append(&payload) {
            Ok(_) => aggregator_metrics().replication_lag.set(0.0),
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

    aggregator_metrics().bulk_export_total.inc();

    #[cfg(feature = "s3")]
    let bucket = S3_BUCKET.as_ref().cloned();
    #[cfg(not(feature = "s3"))]
    let bucket: Option<String> = None;

    let handle = spawn_blocking(move || build_export_payload(map, recipient, password, bucket));
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
            aggregator_metrics().telemetry_ingest_total.inc();
            state.record_telemetry(entry);
            Ok(Response::new(StatusCode::ACCEPTED))
        }
        Err(err) => {
            aggregator_metrics().telemetry_schema_error_total.inc();
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

pub fn router(state: AppState) -> Router<AppState> {
    Router::new(state)
        .post("/ingest", ingest)
        .get("/peer/:id", peer)
        .get("/correlations/:metric", correlations)
        .get("/cluster", cluster)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::encryption::{
        envelope::{self, PASSWORD_CONTENT_TYPE, RECIPIENT_CONTENT_TYPE},
        x25519::SecretKey,
    };
    use crypto_suite::hashing::blake3;
    use foundation_telemetry::WrapperMetricEntry;
    use httpd::{Method, StatusCode};
    use rand::rngs::OsRng;
    use std::collections::HashMap;
    use std::future::Future;
    use sys::archive::zip::ZipReader;
    use sys::tempfile;

    fn run_async<T>(future: impl Future<Output = T>) -> T {
        runtime::block_on(future)
    }

    fn parse_json(input: &str) -> Value {
        json::value_from_str(input).expect("valid test json")
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
            assert_eq!(archive.len(), 2);
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
            assert_eq!(archive.len(), 1);
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
            assert_eq!(archive.len(), 1);
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
