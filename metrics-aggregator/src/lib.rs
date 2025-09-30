use age::Encryptor;
#[cfg(feature = "s3")]
use aws_config;
#[cfg(feature = "s3")]
use aws_sdk_s3::{primitives::ByteStream, Client as S3Client};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
#[cfg(feature = "etcd-client")]
use etcd_client::Client;
use futures::StreamExt;
use httpd::{HttpClient, Method};
use once_cell::sync::Lazy;
use openssl::{
    rand::rand_bytes,
    sha::sha256,
    symm::{Cipher, Crypter, Mode},
};
use prometheus::{IntCounter, IntGauge, Registry, TextEncoder};
use runtime::sync::mpsc::{self, ReceiverStream};
use runtime::{spawn, spawn_blocking};
use tracing::{info, warn};
use urlencoding::encode;

#[cfg(feature = "s3")]
fn upload_sync(bucket: String, data: Vec<u8>) {
    let handle = runtime::handle();
    handle.block_on(async move {
        let config = aws_config::load_from_env().await;
        let client = S3Client::new(&config);
        let _ = client
            .put_object()
            .bucket(bucket)
            .key("metrics/latest.zip")
            .body(ByteStream::from(data))
            .send()
            .await;
    });
}

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct MemorySnapshotEntry {
    latest: u64,
    p50: u64,
    p90: u64,
    p99: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct TelemetrySummaryEntry {
    node_id: String,
    seq: u64,
    timestamp: u64,
    sample_rate_ppm: u64,
    compaction_secs: u64,
    memory: HashMap<String, MemorySnapshotEntry>,
    #[serde(default)]
    wrappers: WrapperSummaryEntry,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct WrapperMetricEntry {
    metric: String,
    labels: HashMap<String, String>,
    value: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct WrapperSummaryEntry {
    metrics: Vec<WrapperMetricEntry>,
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
    pub data: Arc<Mutex<HashMap<String, VecDeque<(u64, serde_json::Value)>>>>,
    pub token: Arc<RwLock<String>>,
    token_path: Option<PathBuf>,
    db: sled::Db,
    retention_secs: u64,
    max_export_peers: usize,
    wal: Option<Arc<Wal>>,
    correlations: Arc<Mutex<HashMap<String, VecDeque<CorrelationRecord>>>>,
    last_metric_values: Arc<Mutex<HashMap<(String, String), f64>>>,
    telemetry: Arc<Mutex<HashMap<String, VecDeque<TelemetrySummaryEntry>>>>,
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
        let db = sled::open(path).expect("open db");
        let mut data = HashMap::new();
        for item in db.iter() {
            if let Ok((k, v)) = item {
                if let Ok(deque) = serde_json::from_slice(&v) {
                    if let Ok(key) = String::from_utf8(k.to_vec()) {
                        data.insert(key, deque);
                    }
                }
            }
        }
        let wal = wal.and_then(|p| Wal::open(p).ok()).map(Arc::new);
        let state = Self {
            data: Arc::new(Mutex::new(data)),
            token: Arc::new(RwLock::new(token)),
            token_path,
            db,
            retention_secs,
            max_export_peers: 1000,
            wal,
            correlations: Arc::new(Mutex::new(HashMap::new())),
            last_metric_values: Arc::new(Mutex::new(HashMap::new())),
            telemetry: Arc::new(Mutex::new(HashMap::new())),
        };
        state.prune();
        state
    }

    fn persist(&self) {
        let _ = self.db.flush();
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
                    let _ = self.db.remove(peer);
                    false
                } else {
                    let _ = self.db.insert(peer, serde_json::to_vec(deque).unwrap());
                    true
                }
            });
        }
        if removed > 0 {
            RETENTION_PRUNED_TOTAL.inc_by(removed);
            let _ = self.db.flush();
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
    fn record_telemetry(&self, entry: TelemetrySummaryEntry) {
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

    fn telemetry_latest(&self) -> HashMap<String, TelemetrySummaryEntry> {
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

    fn telemetry_history(&self, node: &str) -> Vec<TelemetrySummaryEntry> {
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
}

static INGEST_TOTAL: Lazy<IntCounter> =
    Lazy::new(|| IntCounter::new("aggregator_ingest_total", "Total peer metric ingests").unwrap());

static BULK_EXPORT_TOTAL: Lazy<IntCounter> =
    Lazy::new(|| IntCounter::new("bulk_export_total", "Total bulk export attempts").unwrap());

static ACTIVE_PEERS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "cluster_peer_active_total",
        "Unique peers tracked by aggregator",
    )
    .unwrap()
});

static REPLICATION_LAG: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "aggregator_replication_lag_seconds",
        "Seconds since last WAL entry applied",
    )
    .unwrap()
});

static RETENTION_PRUNED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "aggregator_retention_pruned_total",
        "Peer metric samples pruned by retention",
    )
    .unwrap()
});

static REGISTRY: Lazy<Registry> = Lazy::new(|| {
    let r = Registry::new();
    r.register(Box::new(INGEST_TOTAL.clone())).unwrap();
    r.register(Box::new(ACTIVE_PEERS.clone())).unwrap();
    r.register(Box::new(BULK_EXPORT_TOTAL.clone())).unwrap();
    r.register(Box::new(REPLICATION_LAG.clone())).unwrap();
    r.register(Box::new(RETENTION_PRUNED_TOTAL.clone()))
        .unwrap();
    r
});

fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
    use serde_json::{Map, Value};
    match b {
        Value::Object(bm) => {
            if !a.is_object() {
                *a = Value::Object(Map::new());
            }
            let am = a.as_object_mut().unwrap();
            for (k, bv) in bm {
                merge(am.entry(k.clone()).or_insert(Value::Null), bv);
            }
        }
        Value::Number(bn) => {
            let sum = a.as_f64().unwrap_or(0.0) + bn.as_f64().unwrap_or(0.0);
            *a = Value::from(sum);
        }
        _ => {
            *a = b.clone();
        }
    }
}

fn collect_correlations(value: &serde_json::Value) -> Vec<RawCorrelation> {
    fn walk(value: &serde_json::Value, metric: Option<&str>, out: &mut Vec<RawCorrelation>) {
        match value {
            serde_json::Value::Object(map) => {
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
            serde_json::Value::Array(items) => {
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
    let client = HttpClient::default();
    let base = api.trim_end_matches('/');
    let url = format!(
        "{}/logs/search?db={}&correlation={}&limit=50",
        base,
        encode(&db),
        encode(&record.correlation_id)
    );
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

async fn ingest(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<Vec<PeerStat>>,
) -> StatusCode {
    let token = state.current_token();
    if headers
        .get("x-auth-token")
        .and_then(|h| h.to_str().ok())
        .map(|h| h == token)
        .unwrap_or(false)
    {
        let mut map = state.data.lock().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for stat in payload.clone() {
            let entry = map
                .entry(stat.peer_id.clone())
                .or_insert_with(VecDeque::new);
            if let Some((ts, last)) = entry.back_mut() {
                if *ts == now {
                    merge(last, &stat.metrics);
                    let _ = state
                        .db
                        .insert(&stat.peer_id, serde_json::to_vec(entry).unwrap());
                    let correlations = collect_correlations(&stat.metrics);
                    for raw in correlations {
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
            entry.push_back((now, stat.metrics));
            if entry.len() > 1024 {
                entry.pop_front();
            }
            let _ = state
                .db
                .insert(&stat.peer_id, serde_json::to_vec(entry).unwrap());
            if let Some((_, metrics_value)) = entry.back() {
                let correlations = collect_correlations(metrics_value);
                for raw in correlations {
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
        ACTIVE_PEERS.set(map.len() as i64);
        INGEST_TOTAL.inc();
        drop(map);
        state.prune();
        state.persist();
        if let Some(wal) = &state.wal {
            let _ = wal.append(&payload);
            REPLICATION_LAG.set(0);
        }
        if let Ok(blob) = serde_json::to_string(&payload) {
            archive_metrics(&blob);
        }
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn peer(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> Json<Vec<(u64, serde_json::Value)>> {
    let map = state.data.lock().unwrap();
    let data = map
        .get(&id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    #[cfg(feature = "s3")]
    if let Some(b) = &_bucket {
        upload_sync(b.clone(), data.clone());
    }
    Json(data)
}

async fn correlations(
    AxumPath(metric): AxumPath<String>,
    State(state): State<AppState>,
) -> Json<Vec<CorrelationRecord>> {
    Json(state.correlations_for(&metric))
}

async fn cluster(State(state): State<AppState>) -> Json<usize> {
    let map = state.data.lock().unwrap();
    Json(map.len())
}

#[derive(Deserialize)]
struct ExportAllQuery {
    recipient: Option<String>,
    password: Option<String>,
}

async fn export_all(
    State(state): State<AppState>,
    Query(params): Query<ExportAllQuery>,
) -> Result<Response, StatusCode> {
    if params.recipient.is_some() && params.password.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let map = {
        let map = state.data.lock().unwrap();
        if map.len() > state.max_export_peers {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        map.clone()
    };

    BULK_EXPORT_TOTAL.inc();

    let recipient = params.recipient.clone();
    let password = params.password.clone();
    #[cfg(feature = "s3")]
    let bucket = std::env::var("S3_BUCKET").ok();
    #[cfg(not(feature = "s3"))]
    let bucket: Option<String> = None;
    let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
    let _ = spawn_blocking(move || {
        let _bucket = bucket;
        use zip::write::FileOptions;
        let mut cursor = io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for (peer_id, deque) in map {
                let json = serde_json::to_vec(&deque).unwrap();
                let name = format!("{peer_id}.json");
                writer.start_file(name, FileOptions::default()).unwrap();
                writer.write_all(&json).unwrap();
            }
            let _ = writer.finish();
        }
        let bytes = cursor.into_inner();
        let data = if let Some(rec) = recipient {
            use std::str::FromStr;
            let recipient = age::x25519::Recipient::from_str(&rec).unwrap();
            let encryptor = Encryptor::with_recipients(vec![
                Box::new(recipient) as Box<dyn age::Recipient + Send>
            ])
            .unwrap();
            let mut out = Vec::new();
            let mut w = encryptor.wrap_output(&mut out).unwrap();
            w.write_all(&bytes).unwrap();
            w.finish().unwrap();
            out
        } else if let Some(pass) = password {
            let mut iv = [0u8; 16];
            rand_bytes(&mut iv).unwrap();
            let key = sha256(pass.as_bytes());
            let cipher = Cipher::aes_256_cbc();
            let mut crypter = Crypter::new(cipher, Mode::Encrypt, &key, Some(&iv)).unwrap();
            let mut out = vec![0u8; bytes.len() + cipher.block_size()];
            let mut count = crypter.update(&bytes, &mut out).unwrap();
            count += crypter.finalize(&mut out[count..]).unwrap();
            out.truncate(count);
            let mut combined = Vec::with_capacity(iv.len() + out.len());
            combined.extend_from_slice(&iv);
            combined.extend_from_slice(&out);
            combined
        } else {
            bytes
        };
        #[cfg(feature = "s3")]
        if let Some(b) = &_bucket {
            upload_sync(b.clone(), data.clone());
        }
        for chunk in data.chunks(8192) {
            if tx.blocking_send(chunk.to_vec()).is_err() {
                return;
            }
        }
    });

    let body_stream =
        ReceiverStream::new(rx).map(|chunk| Ok::<Bytes, io::Error>(Bytes::from(chunk)));
    let body = axum::body::Body::from_stream(body_stream);
    let mut resp = Response::new(body);
    if params.recipient.is_some() {
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/age"),
        );
    } else if params.password.is_some() {
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/octet-stream"),
        );
    } else {
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/zip"),
        );
    }
    Ok(resp)
}

async fn telemetry_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(entry): Json<TelemetrySummaryEntry>,
) -> StatusCode {
    let token = state.current_token();
    let authorized = headers
        .get("x-auth-token")
        .and_then(|h| h.to_str().ok())
        .map(|h| h == token)
        .unwrap_or(false);
    if authorized {
        state.record_telemetry(entry);
        StatusCode::ACCEPTED
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn telemetry_index(
    State(state): State<AppState>,
) -> Json<HashMap<String, TelemetrySummaryEntry>> {
    Json(state.telemetry_latest())
}

async fn telemetry_node(
    State(state): State<AppState>,
    AxumPath(node): AxumPath<String>,
) -> Json<Vec<TelemetrySummaryEntry>> {
    Json(state.telemetry_history(&node))
}

async fn wrappers(State(state): State<AppState>) -> Json<HashMap<String, WrapperSummaryEntry>> {
    Json(state.wrappers_latest())
}

async fn metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = String::new();
    encoder.encode_utf8(&metric_families, &mut buffer).unwrap();
    buffer
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ingest", post(ingest))
        .route("/peer/:id", get(peer))
        .route("/correlations/:metric", get(correlations))
        .route("/cluster", get(cluster))
        .route("/telemetry", post(telemetry_post).get(telemetry_index))
        .route("/telemetry/:node", get(telemetry_node))
        .route("/wrappers", get(wrappers))
        .route("/export/all", get(export_all))
        .route("/healthz", get(health))
        .route("/metrics", get(metrics))
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

struct Wal {
    file: Mutex<std::fs::File>,
}

#[cfg(feature = "etcd-client")]
#[allow(dead_code)]
pub async fn run_leader_election(endpoints: Vec<String>, state: AppState) {
    if let Ok(mut client) = Client::connect(endpoints, None).await {
        if let Ok(resp) = client.lease_grant(5, None).await {
            let lease_id = resp.id();
            if client
                .put("metrics-aggregator/leader", "", Some(lease_id))
                .await
                .is_ok()
            {
                let _ = client.lease_keep_alive(lease_id).await;
            }
        }
    }
    let _ = state; // suppress unused warning when feature disabled
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
        let line = serde_json::to_vec(stats)?;
        guard.write_all(&line)?;
        guard.write_all(b"\n")?;
        guard.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{self, Body};
    use axum::http::{Request, StatusCode};
    use std::collections::HashMap;
    use std::future::Future;
    use std::io::Cursor;
    use tempfile::tempdir;
    use tower::ServiceExt; // for `oneshot`
    use zip::ZipArchive;

    fn run_async<T>(future: impl Future<Output = T>) -> T {
        runtime::block_on(future)
    }

    #[test]
    fn dedupes_by_peer() {
        run_async(async {
            let dir = tempdir().unwrap();
            let state = AppState::new("token".into(), dir.path().join("m.json"), 60);
            let app = router(state.clone());
            let payload = serde_json::json!([
                {"peer_id": "a", "metrics": {"r":1}},
                {"peer_id": "a", "metrics": {"r":2}}
            ]);
            let req = Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("content-type", "application/json")
                .header("x-auth-token", "token")
                .body(Body::from(payload.to_string()))
                .unwrap();
            let _ = app.clone().oneshot(req).await.unwrap();
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/peer/a")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let vals: Vec<(u64, serde_json::Value)> = serde_json::from_slice(&body).unwrap();
            assert_eq!(vals.len(), 1);
            assert_eq!(vals[0].1["r"].as_f64().unwrap() as i64, 3);
        });
    }

    #[test]
    fn persists_and_prunes() {
        run_async(async {
            let dir = tempdir().unwrap();
            let path = dir.path().join("m.json");
            {
                let state = AppState::new("t".into(), &path, 1);
                let app = router(state.clone());
                let payload = serde_json::json!([{ "peer_id": "p", "metrics": {"v": 1}}]);
                let req = Request::builder()
                    .method("POST")
                    .uri("/ingest")
                    .header("content-type", "application/json")
                    .header("x-auth-token", "t")
                    .body(Body::from(payload.to_string()))
                    .unwrap();
                let _ = app.oneshot(req).await.unwrap();
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
            let dir = tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = serde_json::json!([{ "peer_id": "p1", "metrics": {"v": 1}}, {"peer_id": "p2", "metrics": {"v": 2}}]);
                let req = Request::builder()
                    .method("POST")
                    .uri("/ingest")
                    .header("content-type", "application/json")
                    .header("x-auth-token", "t")
                    .body(Body::from(payload.to_string()))
                    .unwrap();
                let _ = app.oneshot(req).await.unwrap();
            }
            let app = router(state);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/export/all")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body_bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let hash = blake3::hash(&body_bytes);
            assert_ne!(hash.as_bytes(), &[0u8; 32]);
            let cursor = Cursor::new(body_bytes);
            let mut zip = ZipArchive::new(cursor).unwrap();
            assert_eq!(zip.len(), 2);
            let mut file = zip.by_name("p1.json").unwrap();
            let mut contents = String::new();
            use std::io::Read;
            file.read_to_string(&mut contents).unwrap();
            let v: Vec<(u64, serde_json::Value)> = serde_json::from_str(&contents).unwrap();
            assert_eq!(v[0].1["v"].as_i64().unwrap(), 1);
        });
    }

    #[test]
    fn export_all_encrypts() {
        use age::{x25519::Identity, Decryptor};
        run_async(async {
            let dir = tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = serde_json::json!([{ "peer_id": "p1", "metrics": {"v": 1}}]);
                let req = Request::builder()
                    .method("POST")
                    .uri("/ingest")
                    .header("content-type", "application/json")
                    .header("x-auth-token", "t")
                    .body(Body::from(payload.to_string()))
                    .unwrap();
                let _ = app.oneshot(req).await.unwrap();
            }
            let id = Identity::generate();
            let recipient = id.to_public().to_string();
            let app = router(state);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri(format!("/export/all?recipient={}", recipient))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(
                resp.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/age"
            );
            let body_bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let decryptor = Decryptor::new(&body_bytes[..]).unwrap();
            let mut plain = Vec::new();
            if let Decryptor::Recipients(d) = decryptor {
                use std::io::Read;
                let mut r = d
                    .decrypt(std::iter::once(&id as &dyn age::Identity))
                    .unwrap();
                r.read_to_end(&mut plain).unwrap();
            } else {
                panic!();
            }
            let mut zip = ZipArchive::new(Cursor::new(plain)).unwrap();
            assert_eq!(zip.len(), 1);
            let mut file = zip.by_name("p1.json").unwrap();
            let mut contents = String::new();
            use std::io::Read;
            file.read_to_string(&mut contents).unwrap();
            let v: Vec<(u64, serde_json::Value)> = serde_json::from_str(&contents).unwrap();
            assert_eq!(v[0].1["v"].as_i64().unwrap(), 1);
        });
    }

    #[test]
    fn export_all_openssl_encrypts() {
        use openssl::symm::{decrypt, Cipher};
        run_async(async {
            let dir = tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            {
                let app = router(state.clone());
                let payload = serde_json::json!([{ "peer_id": "p1", "metrics": {"v": 1}}]);
                let req = Request::builder()
                    .method("POST")
                    .uri("/ingest")
                    .header("content-type", "application/json")
                    .header("x-auth-token", "t")
                    .body(Body::from(payload.to_string()))
                    .unwrap();
                let _ = app.oneshot(req).await.unwrap();
            }
            let app = router(state);
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/export/all?password=secret")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(
                resp.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/octet-stream"
            );
            let body_bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let (iv, cipher) = body_bytes.split_at(16);
            let key = sha256(b"secret");
            let plain = decrypt(Cipher::aes_256_cbc(), &key, Some(iv), cipher).unwrap();
            let mut zip = ZipArchive::new(Cursor::new(plain)).unwrap();
            assert_eq!(zip.len(), 1);
            let mut file = zip.by_name("p1.json").unwrap();
            let mut contents = String::new();
            use std::io::Read;
            file.read_to_string(&mut contents).unwrap();
            let v: Vec<(u64, serde_json::Value)> = serde_json::from_str(&contents).unwrap();
            assert_eq!(v[0].1["v"].as_i64().unwrap(), 1);
        });
    }

    #[test]
    fn wrappers_endpoint_returns_latest_metrics() {
        run_async(async {
            let dir = tempdir().unwrap();
            let state = AppState::new("t".into(), dir.path().join("m.json"), 60);
            state.record_telemetry(TelemetrySummaryEntry {
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
                .oneshot(
                    Request::builder()
                        .uri("/wrappers")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(resp.status(), StatusCode::OK);
            let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let parsed: HashMap<String, WrapperSummaryEntry> =
                serde_json::from_slice(&body).unwrap();
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
