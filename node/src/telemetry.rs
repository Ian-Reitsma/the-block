#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::net;
use crate::py::{PyError, PyResult};
#[cfg(feature = "telemetry")]
use codec::{self, Codec, Direction};
#[cfg(feature = "telemetry")]
use concurrency::DashMap;
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use crypto_suite::hashing::blake3;
#[cfg(feature = "telemetry")]
use crypto_suite::{self, signatures::ed25519};
#[cfg(feature = "telemetry")]
use diagnostics::internal::{
    install_tls_env_warning_subscriber, SubscriberGuard as DiagnosticsSubscriberGuard,
};
#[cfg(feature = "telemetry")]
use foundation_metrics::{self, Recorder as MetricsRecorder};
#[cfg(feature = "telemetry")]
use histogram_fp::Histogram as HdrHistogram;
use httpd::{BlockingClient, Method};
#[cfg(feature = "telemetry")]
use rand::Rng;
use runtime::telemetry::{
    self, Encoder, GaugeVec, Histogram, HistogramHandle, HistogramOpts, HistogramVec, IntCounter,
    IntCounterHandle, IntCounterVec, IntGauge, IntGaugeHandle, IntGaugeVec, Opts, Registry,
    TextEncoder,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(feature = "telemetry"))]
use std::sync::Once;
#[cfg(feature = "telemetry")]
use std::sync::RwLock;
#[cfg(feature = "telemetry")]
use std::sync::{Mutex, Once};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(feature = "telemetry")]
use sys::process;

use foundation_serialization::Serialize;
use tls_warning::WarningOrigin;
#[cfg(feature = "telemetry")]
use tls_warning::{
    detail_fingerprint as tls_detail_fingerprint, dispatch_tls_env_warning_event,
    fingerprint_label, reset_tls_env_warning_telemetry_sinks_for_test,
    variables_fingerprint as tls_variables_fingerprint,
};
#[cfg(not(feature = "telemetry"))]
use tls_warning::{
    detail_fingerprint as tls_detail_fingerprint, fingerprint_label,
    variables_fingerprint as tls_variables_fingerprint,
};
#[cfg(feature = "telemetry")]
pub use tls_warning::{
    register_tls_env_warning_telemetry_sink, TlsEnvWarningTelemetryEvent,
    TlsEnvWarningTelemetrySinkGuard,
};

#[cfg(not(feature = "telemetry"))]
#[derive(Clone, Debug)]
pub struct TlsEnvWarningTelemetryEvent;

#[cfg(not(feature = "telemetry"))]
pub struct TlsEnvWarningTelemetrySinkGuard;

#[cfg(not(feature = "telemetry"))]
pub fn register_tls_env_warning_telemetry_sink<F>(_sink: F) -> TlsEnvWarningTelemetrySinkGuard
where
    F: Fn(&TlsEnvWarningTelemetryEvent) + Send + Sync + 'static,
{
    TlsEnvWarningTelemetrySinkGuard
}

#[cfg(feature = "telemetry")]
#[derive(Clone, Debug, Serialize)]
pub struct TlsEnvWarningSnapshot {
    pub prefix: String,
    pub code: String,
    pub total: u64,
    pub last_delta: u64,
    pub last_seen: u64,
    pub origin: WarningOrigin,
    pub detail: Option<String>,
    pub detail_fingerprint: Option<i64>,
    pub variables: Vec<String>,
    pub variables_fingerprint: Option<i64>,
    pub detail_fingerprint_counts: BTreeMap<String, u64>,
    pub variables_fingerprint_counts: BTreeMap<String, u64>,
}

#[cfg(not(feature = "telemetry"))]
#[derive(Clone, Debug)]
pub struct TlsEnvWarningSnapshot;

#[cfg(feature = "telemetry")]
#[derive(Default)]
struct LocalTlsWarning {
    total: u64,
    last_delta: u64,
    last_seen: u64,
    origin: WarningOrigin,
    detail: Option<String>,
    variables: Vec<String>,
    detail_fingerprint: Option<i64>,
    variables_fingerprint: Option<i64>,
    detail_fingerprint_counts: BTreeMap<String, u64>,
    variables_fingerprint_counts: BTreeMap<String, u64>,
}

#[cfg(feature = "telemetry")]
static TLS_ENV_WARNINGS: Lazy<DashMap<(String, String), LocalTlsWarning>> = Lazy::new(DashMap::new);

static GOV_WEBHOOK_CLIENT: Lazy<BlockingClient> =
    Lazy::new(|| crate::http_client::blocking_client());

#[derive(Serialize)]
struct GovernanceWebhookPayload<'a> {
    event: &'a str,
    proposal_id: u64,
}

pub mod summary;

#[cfg(feature = "telemetry")]
pub use bridges::{
    BRIDGE_CHALLENGES_TOTAL, BRIDGE_DISPUTE_OUTCOMES_TOTAL, BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL,
    BRIDGE_REWARD_CLAIMS_TOTAL, BRIDGE_SETTLEMENT_RESULTS_TOTAL, BRIDGE_SLASHES_TOTAL,
};

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

pub const LABEL_REGISTRATION_ERR: &str = "telemetry label set not registered";

#[cfg(feature = "telemetry")]
static SAMPLE_RATE: AtomicU64 = AtomicU64::new(1_000_000); // parts per million
#[cfg(feature = "telemetry")]
static BASE_SAMPLE_RATE: AtomicU64 = AtomicU64::new(1_000_000);
#[cfg(feature = "telemetry")]
static SAMPLE_FAIL_EVENTS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "telemetry")]
static COMPACTION_SECS: AtomicU64 = AtomicU64::new(60);
#[cfg(feature = "telemetry")]
static COMPACTOR: Once = Once::new();
#[cfg(feature = "telemetry")]
static WRAPPER_INIT: Once = Once::new();
#[cfg(feature = "telemetry")]
static CODING_PREVIOUS: Lazy<Mutex<HashMap<&'static str, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "telemetry")]
const KNOWN_RUNTIME_BACKENDS: [&str; 2] = ["inhouse", "stub"];
#[cfg(feature = "telemetry")]
const KNOWN_TRANSPORT_PROVIDERS: [&str; 2] = ["quinn", "s2n-quic"];
#[cfg(feature = "telemetry")]
const KNOWN_STORAGE_ENGINES: [&str; 4] = ["memory", "inhouse", "rocksdb", "rocksdb-compat"];
#[cfg(feature = "telemetry")]
const KNOWN_CODEC_PROFILES: &[(&str, &[&str])] = &[
    (
        "binary",
        &["canonical", "transaction", "gossip", "storage_manifest"],
    ),
    ("json", &["none"]),
];
#[cfg(all(feature = "quic", feature = "s2n-quic"))]
const COMPILED_TRANSPORT_PROVIDERS: [&str; 2] = ["quinn", "s2n-quic"];
#[cfg(all(feature = "quic", not(feature = "s2n-quic")))]
const COMPILED_TRANSPORT_PROVIDERS: [&str; 1] = ["quinn"];
#[cfg(not(feature = "quic"))]
const COMPILED_TRANSPORT_PROVIDERS: [&str; 0] = [];

#[cfg(feature = "telemetry")]
const ADAPTIVE_MIN_SAMPLE_RATE: u64 = 10_000; // 1%
#[cfg(feature = "telemetry")]
const ADAPTIVE_WINDOW_SECS: u64 = 30;

#[cfg(feature = "telemetry")]
static ADAPTIVE_LOOP: Lazy<()> = Lazy::new(|| {
    std::thread::spawn(adaptive_sampling_loop);
});

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MemoryComponent {
    Mempool,
    Storage,
    Compute,
}

impl MemoryComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryComponent::Mempool => "mempool",
            MemoryComponent::Storage => "storage",
            MemoryComponent::Compute => "compute",
        }
    }
}

#[cfg_attr(feature = "telemetry", derive(Serialize))]
#[derive(Clone, Copy, Default, Debug)]
pub struct MemorySnapshot {
    pub latest: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
}

#[cfg(feature = "telemetry")]
struct MemoryHist {
    hist: Mutex<HdrHistogram>,
    latest: AtomicU64,
}

#[cfg(feature = "telemetry")]
impl Default for MemoryHist {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "telemetry")]
impl MemoryHist {
    fn new() -> Self {
        Self {
            hist: Mutex::new(HdrHistogram::new_with_bounds(1, 1 << 42, 3).unwrap()),
            latest: AtomicU64::new(0),
        }
    }

    fn observe(&self, value: u64) {
        self.latest.store(value, Ordering::Relaxed);
        if let Ok(mut guard) = self.hist.lock() {
            let _ = guard.record(value);
        }
    }

    fn reset(&self) {
        if let Ok(mut guard) = self.hist.lock() {
            guard.reset();
        }
    }

    fn snapshot(&self) -> MemorySnapshot {
        let latest = self.latest.load(Ordering::Relaxed);
        if let Ok(mut guard) = self.hist.lock() {
            if guard.len() == 0 {
                MemorySnapshot {
                    latest,
                    ..MemorySnapshot::default()
                }
            } else {
                MemorySnapshot {
                    latest,
                    p50: guard.value_at_percentile(50.0),
                    p90: guard.value_at_percentile(90.0),
                    p99: guard.value_at_percentile(99.0),
                }
            }
        } else {
            MemorySnapshot {
                latest,
                ..MemorySnapshot::default()
            }
        }
    }
}

/// In-memory read metrics aggregated per domain.
#[cfg(feature = "telemetry")]
#[derive(Default)]
pub struct ReadStats {
    inner: DashMap<String, ReadStat>,
    memory: DashMap<MemoryComponent, MemoryHist>,
}

#[cfg(feature = "telemetry")]
#[derive(Clone, Debug, Serialize)]
pub struct WrapperMetricSample {
    pub metric: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
}

#[cfg_attr(feature = "telemetry", derive(Serialize))]
#[derive(Clone, Debug, Default)]
pub struct WrapperSummary {
    #[cfg(feature = "telemetry")]
    pub metrics: Vec<WrapperMetricSample>,
}

#[cfg(feature = "telemetry")]
#[derive(Default)]
struct ReadStat {
    reads: AtomicU64,
    bytes: AtomicU64,
}

#[cfg(feature = "telemetry")]
impl ReadStats {
    pub fn new() -> Self {
        let stats = Self {
            inner: DashMap::new(),
            memory: DashMap::new(),
        };
        for component in [
            MemoryComponent::Mempool,
            MemoryComponent::Storage,
            MemoryComponent::Compute,
        ] {
            stats
                .memory
                .entry(component)
                .or_insert_with(MemoryHist::new);
        }
        stats
    }

    pub fn record(&self, domain: &str, bytes: u64) {
        let stat = self
            .inner
            .entry(domain.to_owned())
            .or_insert_with(ReadStat::default);
        stat.reads.fetch_add(1, Ordering::Relaxed);
        stat.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn snapshot(&self, domain: &str) -> (u64, u64) {
        let key = domain.to_owned();
        self.inner
            .get(&key)
            .map(|s| {
                (
                    s.reads.load(Ordering::Relaxed),
                    s.bytes.load(Ordering::Relaxed),
                )
            })
            .unwrap_or((0, 0))
    }

    pub fn record_memory(&self, component: MemoryComponent, bytes: u64) {
        let entry = self.memory.entry(component).or_insert_with(MemoryHist::new);
        entry.observe(bytes);
    }

    pub fn memory_snapshot(&self, component: MemoryComponent) -> Option<MemorySnapshot> {
        self.memory.get(&component).map(|h| h.snapshot())
    }

    pub fn memory_snapshot_all(&self) -> HashMap<&'static str, MemorySnapshot> {
        let mut snapshots = HashMap::new();
        self.memory.for_each(|component, hist| {
            snapshots.insert(component.as_str(), hist.snapshot());
        });
        snapshots
    }

    pub fn reset_memory(&self) {
        self.memory.for_each_mut(|_, hist| hist.reset());
    }
}

#[cfg(not(feature = "telemetry"))]
#[derive(Default)]
pub struct ReadStats;

#[cfg(not(feature = "telemetry"))]
impl ReadStats {
    pub fn new() -> Self {
        Self
    }
    pub fn record(&self, _domain: &str, _bytes: u64) {}
    pub fn snapshot(&self, _domain: &str) -> (u64, u64) {
        (0, 0)
    }
    pub fn record_memory(&self, _component: MemoryComponent, _bytes: u64) {}
    pub fn memory_snapshot(&self, _component: MemoryComponent) -> Option<MemorySnapshot> {
        None
    }
    pub fn memory_snapshot_all(&self) -> HashMap<&'static str, MemorySnapshot> {
        HashMap::new()
    }
    pub fn reset_memory(&self) {}
}

#[cfg(feature = "telemetry")]
pub static READ_STATS: Lazy<ReadStats> = Lazy::new(ReadStats::new);
#[cfg(not(feature = "telemetry"))]
pub static READ_STATS: ReadStats = ReadStats;

#[cfg(feature = "telemetry")]
fn should_sample() -> bool {
    Lazy::force(&ADAPTIVE_LOOP);
    let rate = SAMPLE_RATE.load(Ordering::Relaxed);
    if rate >= 1_000_000 {
        return true;
    }
    rand::thread_rng().gen_range(0..1_000_000u64) < rate
}

#[cfg(feature = "telemetry")]
fn sample_weight() -> u64 {
    let rate = SAMPLE_RATE.load(Ordering::Relaxed);
    if rate == 0 {
        0
    } else {
        (1_000_000f64 / rate as f64).round() as u64
    }
}

#[cfg(feature = "telemetry")]
pub fn sampled_inc(counter: &IntCounterHandle) {
    if should_sample() {
        counter.inc_by(sample_weight());
    }
}

#[cfg(feature = "telemetry")]
pub fn sampled_inc_vec(counter: &IntCounterVec, labels: &[&str]) {
    if should_sample() {
        counter
            .ensure_handle_for_label_values(labels)
            .expect(LABEL_REGISTRATION_ERR)
            .inc_by(sample_weight());
    }
}

#[cfg(feature = "telemetry")]
pub fn sampled_observe(hist: &HistogramHandle, v: f64) {
    if should_sample() {
        hist.observe(v);
    }
}

#[cfg(feature = "telemetry")]
pub fn sampled_observe_vec(hist: &HistogramVec, labels: &[&str], v: f64) {
    if should_sample() {
        hist.ensure_handle_for_label_values(labels)
            .expect(LABEL_REGISTRATION_ERR)
            .observe(v);
    }
}

#[cfg(feature = "telemetry")]
pub fn record_coding_result(stage: &str, algorithm: &str, result: &str) {
    STORAGE_CODING_OPERATIONS_TOTAL
        .ensure_handle_for_label_values(&[stage, algorithm, result])
        .expect(LABEL_REGISTRATION_ERR)
        .inc();
}

#[cfg(feature = "telemetry")]
const CODEC_NONE_PROFILE: &str = "none";

#[cfg(feature = "telemetry")]
fn compiled_transport_providers() -> &'static [&'static str] {
    &COMPILED_TRANSPORT_PROVIDERS
}

#[cfg(feature = "telemetry")]
fn set_coding_metric(component: &str, algorithm: &str, mode: &str, value: i64) {
    CODING_ALGORITHM_INFO
        .ensure_handle_for_label_values(&[component, algorithm, mode])
        .expect(LABEL_REGISTRATION_ERR)
        .set(value);
}

#[cfg(feature = "telemetry")]
fn codec_labels(codec: Codec) -> (&'static str, &'static str) {
    match codec {
        Codec::Binary(profile) => (
            "binary",
            match profile {
                codec::BinaryProfile::Canonical => "canonical",
                codec::BinaryProfile::Transaction => "transaction",
                codec::BinaryProfile::Gossip => "gossip",
                codec::BinaryProfile::StorageManifest => "storage_manifest",
            },
        ),
        Codec::Json(_) => ("json", CODEC_NONE_PROFILE),
    }
}

#[cfg(feature = "telemetry")]
fn direction_label(direction: Direction) -> &'static str {
    match direction {
        Direction::Serialize => "serialize",
        Direction::Deserialize => "deserialize",
    }
}

#[cfg(feature = "telemetry")]
fn reset_coding_component(map: &mut HashMap<&'static str, String>, component: &'static str) {
    if let Some(prev) = map.get(component) {
        set_coding_metric(component, prev, "active", 0);
        set_coding_metric(component, prev, "fallback", 0);
        set_coding_metric(component, prev, "emergency", 0);
    }
}

#[cfg(feature = "telemetry")]
fn push_metric(
    out: &mut Vec<WrapperMetricSample>,
    metric: &str,
    labels: &[(&str, &str)],
    value: f64,
) {
    let mut map = HashMap::new();
    for (k, v) in labels {
        map.insert((*k).to_string(), (*v).to_string());
    }
    out.push(WrapperMetricSample {
        metric: metric.to_string(),
        labels: map,
        value,
    });
}

#[cfg(feature = "telemetry")]
fn install_foundation_metrics_recorder() {
    static RECORDER_INSTALL: Once = Once::new();
    RECORDER_INSTALL.call_once(|| {
        if let Err(err) = foundation_metrics::install_recorder(NodeMetricsRecorder) {
            diagnostics::tracing::warn!(
                reason = %err,
                "foundation_metrics_recorder_install_failed"
            );
        }
    });
}

#[cfg(feature = "telemetry")]
fn label_value<'a>(labels: &'a [(String, String)], key: &str) -> Option<&'a str> {
    labels
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(feature = "telemetry")]
fn counter_delta(value: f64) -> u64 {
    if !value.is_finite() {
        0
    } else if value <= 0.0 {
        0
    } else {
        value.round() as u64
    }
}

#[cfg(feature = "telemetry")]
fn gauge_value(value: f64) -> i64 {
    if !value.is_finite() {
        0
    } else {
        value.round() as i64
    }
}

#[cfg(feature = "telemetry")]
#[derive(Debug)]
struct NodeMetricsRecorder;

#[cfg(feature = "telemetry")]
impl MetricsRecorder for NodeMetricsRecorder {
    fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]) {
        match name {
            "codec_operation_fail_total" => {
                let Some(codec) = label_value(labels, "codec") else {
                    return;
                };
                let Some(direction) = label_value(labels, "direction") else {
                    return;
                };
                let profile = label_value(labels, "profile").unwrap_or(CODEC_NONE_PROFILE);
                let delta = counter_delta(value);
                if delta == 0 {
                    return;
                }
                let version = codec::VERSION;
                match direction {
                    "serialize" => {
                        if let Ok(counter) = CODEC_SERIALIZE_FAIL_TOTAL
                            .ensure_handle_for_label_values(&[codec, profile, version])
                        {
                            counter.inc_by(delta);
                        }
                    }
                    "deserialize" => {
                        if let Ok(counter) = CODEC_DESERIALIZE_FAIL_TOTAL
                            .ensure_handle_for_label_values(&[codec, profile, version])
                        {
                            counter.inc_by(delta);
                        }
                    }
                    _ => {}
                }
            }
            "ad_verifier_committee_rejection_total" => {
                let Some(committee) = label_value(labels, "committee") else {
                    return;
                };
                let Some(reason) = label_value(labels, "reason") else {
                    return;
                };
                let delta = counter_delta(value);
                if delta == 0 {
                    return;
                }
                if let Ok(counter) = AD_VERIFIER_COMMITTEE_REJECTION_TOTAL
                    .ensure_handle_for_label_values(&[committee, reason])
                {
                    counter.inc_by(delta);
                }
            }
            "remote_signer_request_total" => {
                let delta = counter_delta(value);
                if delta > 0 {
                    REMOTE_SIGNER_REQUEST_TOTAL.inc_by(delta);
                }
            }
            "remote_signer_success_total" => {
                let delta = counter_delta(value);
                if delta > 0 {
                    REMOTE_SIGNER_SUCCESS_TOTAL.inc_by(delta);
                }
            }
            "remote_signer_key_rotation_total" => {
                let delta = counter_delta(value);
                if delta > 0 {
                    REMOTE_SIGNER_KEY_ROTATION_TOTAL.inc_by(delta);
                }
            }
            "snapshot_restore_fail_total" => {
                let delta = counter_delta(value);
                if delta > 0 {
                    SNAPSHOT_RESTORE_FAIL_TOTAL.inc_by(delta);
                }
            }
            _ => {}
        }
    }

    fn record_histogram(&self, name: &str, value: f64, labels: &[(String, String)]) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        match name {
            "codec_payload_bytes" => {
                let Some(codec) = label_value(labels, "codec") else {
                    return;
                };
                let Some(direction) = label_value(labels, "direction") else {
                    return;
                };
                let profile = label_value(labels, "profile").unwrap_or(CODEC_NONE_PROFILE);
                let version = codec::VERSION;
                if let Ok(hist) = CODEC_PAYLOAD_BYTES
                    .ensure_handle_for_label_values(&[codec, direction, profile, version])
                {
                    hist.observe(value);
                }
            }
            "remote_signer_latency_seconds" => {
                REMOTE_SIGNER_LATENCY_SECONDS.observe(value);
            }
            "runtime_spawn_latency_seconds" => {
                RUNTIME_SPAWN_LATENCY_SECONDS.observe(value);
            }
            _ => {}
        }
    }

    fn record_gauge(&self, name: &str, value: f64, _labels: &[(String, String)]) {
        match name {
            "runtime_pending_tasks" => {
                RUNTIME_PENDING_TASKS.set(gauge_value(value));
            }
            _ => {}
        }
    }
}

#[cfg(feature = "telemetry")]
pub fn init_wrapper_metrics() {
    WRAPPER_INIT.call_once(|| {
        install_foundation_metrics_recorder();
        if let Err(err) = codec::install_metrics_hook(codec_metrics_hook) {
            diagnostics::tracing::warn!(reason = %err, "codec_metrics_hook_install_failed");
        }
        if let Err(err) = ed25519::install_telemetry_hook(crypto_metrics_hook) {
            diagnostics::tracing::warn!(reason = %err, "crypto_metrics_hook_install_failed");
        }
        record_runtime_backend(crate::runtime::handle().backend_name());
        record_crypto_backend();
        record_coding_algorithms(&crate::storage::settings::algorithms());
    });
}

#[cfg(not(feature = "telemetry"))]
pub fn init_wrapper_metrics() {}

#[cfg(feature = "telemetry")]
pub fn record_runtime_backend(active: &str) {
    let compiled = crate::runtime::compiled_backends();
    for backend in KNOWN_RUNTIME_BACKENDS {
        let compiled_flag = if compiled.iter().any(|&b| b == backend) {
            "true"
        } else {
            "false"
        };
        let value = if backend == active { 1 } else { 0 };
        RUNTIME_BACKEND_INFO
            .ensure_handle_for_label_values(&[backend, compiled_flag])
            .expect(LABEL_REGISTRATION_ERR)
            .set(value);
    }
    if !KNOWN_RUNTIME_BACKENDS.contains(&active) {
        RUNTIME_BACKEND_INFO
            .ensure_handle_for_label_values(&[active, "true"])
            .expect(LABEL_REGISTRATION_ERR)
            .set(1);
    }
}

#[cfg(feature = "telemetry")]
pub fn record_dependency_policy(kind: &str, allowed: &[String]) {
    let known = match kind {
        "runtime" => &KNOWN_RUNTIME_BACKENDS[..],
        "transport" => &KNOWN_TRANSPORT_PROVIDERS[..],
        "storage" => &KNOWN_STORAGE_ENGINES[..],
        _ => return,
    };
    for label in known {
        let value = if allowed
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(label))
        {
            1.0
        } else {
            0.0
        };
        GOV_DEPENDENCY_POLICY_ALLOWED
            .ensure_handle_for_label_values(&[kind, label])
            .expect(LABEL_REGISTRATION_ERR)
            .set(value);
    }
    if crate::telemetry::should_log("governance") {
        diagnostics::tracing::info!(kind, allowed = ?allowed, "dependency_policy_recorded");
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn record_runtime_backend(_active: &str) {}

#[cfg(not(feature = "telemetry"))]
pub fn record_dependency_policy(_kind: &str, _allowed: &[String]) {}

#[cfg(feature = "telemetry")]
pub fn record_transport_backend(active: &str) {
    let compiled = compiled_transport_providers();
    for provider in KNOWN_TRANSPORT_PROVIDERS {
        let compiled_flag = if compiled.iter().any(|&p| p == provider) {
            "true"
        } else {
            "false"
        };
        let value = if provider == active { 1 } else { 0 };
        TRANSPORT_PROVIDER_INFO
            .ensure_handle_for_label_values(&[provider, compiled_flag])
            .expect(LABEL_REGISTRATION_ERR)
            .set(value);
    }
    if !KNOWN_TRANSPORT_PROVIDERS.contains(&active) {
        TRANSPORT_PROVIDER_INFO
            .ensure_handle_for_label_values(&[active, "true"])
            .expect(LABEL_REGISTRATION_ERR)
            .set(1);
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn record_transport_backend(_active: &str) {}

#[cfg(feature = "telemetry")]
pub fn record_coding_algorithms(algorithms: &crate::storage::settings::Algorithms) {
    let mut guard = CODING_PREVIOUS.lock().unwrap();

    reset_coding_component(&mut guard, "encryptor");
    set_coding_metric("encryptor", algorithms.encryptor(), "active", 1);
    set_coding_metric("encryptor", algorithms.encryptor(), "fallback", 0);
    set_coding_metric("encryptor", algorithms.encryptor(), "emergency", 0);
    guard.insert("encryptor", algorithms.encryptor().to_string());

    reset_coding_component(&mut guard, "fountain");
    set_coding_metric("fountain", algorithms.fountain(), "active", 1);
    set_coding_metric("fountain", algorithms.fountain(), "fallback", 0);
    set_coding_metric("fountain", algorithms.fountain(), "emergency", 0);
    guard.insert("fountain", algorithms.fountain().to_string());

    reset_coding_component(&mut guard, "erasure");
    let erasure_algo = algorithms.erasure();
    set_coding_metric("erasure", erasure_algo, "active", 1);
    set_coding_metric(
        "erasure",
        erasure_algo,
        "fallback",
        if algorithms.erasure_fallback() { 1 } else { 0 },
    );
    set_coding_metric(
        "erasure",
        erasure_algo,
        "emergency",
        if algorithms.erasure_emergency() { 1 } else { 0 },
    );
    guard.insert("erasure", erasure_algo.to_string());

    reset_coding_component(&mut guard, "compression");
    let compression_algo = algorithms.compression();
    set_coding_metric("compression", compression_algo, "active", 1);
    set_coding_metric(
        "compression",
        compression_algo,
        "fallback",
        if algorithms.compression_fallback() {
            1
        } else {
            0
        },
    );
    set_coding_metric(
        "compression",
        compression_algo,
        "emergency",
        if algorithms.compression_emergency() {
            1
        } else {
            0
        },
    );
    guard.insert("compression", compression_algo.to_string());
}

#[cfg(not(feature = "telemetry"))]
pub fn record_coding_algorithms(_algorithms: &crate::storage::settings::Algorithms) {}

#[cfg(feature = "telemetry")]
pub fn record_crypto_backend() {
    CRYPTO_BACKEND_INFO
        .ensure_handle_for_label_values(&[
            ed25519::ALGORITHM,
            ed25519::BACKEND,
            ed25519::BACKEND_VERSION,
        ])
        .expect(LABEL_REGISTRATION_ERR)
        .set(1);
}

#[cfg(not(feature = "telemetry"))]
pub fn record_crypto_backend() {}

#[cfg(feature = "telemetry")]
fn codec_metrics_hook(codec: Codec, direction: Direction, size: Option<usize>) {
    let (codec_label, profile_label) = codec_labels(codec);
    let dir_label = direction_label(direction);
    match size {
        Some(len) => {
            CODEC_PAYLOAD_BYTES
                .ensure_handle_for_label_values(&[
                    codec_label,
                    dir_label,
                    profile_label,
                    codec::VERSION,
                ])
                .expect(LABEL_REGISTRATION_ERR)
                .observe(len as f64);
        }
        None => {
            let labels = [codec_label, profile_label, codec::VERSION];
            match direction {
                Direction::Serialize => {
                    CODEC_SERIALIZE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&labels)
                        .expect(LABEL_REGISTRATION_ERR)
                        .inc();
                }
                Direction::Deserialize => {
                    CODEC_DESERIALIZE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&labels)
                        .expect(LABEL_REGISTRATION_ERR)
                        .inc();
                }
            }
        }
    }
}

#[cfg(feature = "telemetry")]
fn crypto_metrics_hook(
    algorithm: &'static str,
    operation: &'static str,
    backend: &'static str,
    success: bool,
) {
    let result = if success { "ok" } else { "error" };
    CRYPTO_OPERATION_TOTAL
        .ensure_handle_for_label_values(&[
            algorithm,
            backend,
            ed25519::BACKEND_VERSION,
            operation,
            result,
        ])
        .expect(LABEL_REGISTRATION_ERR)
        .inc();
}

#[cfg(feature = "telemetry")]
pub fn wrapper_metrics_snapshot() -> WrapperSummary {
    let mut metrics = Vec::new();

    let compiled_runtime = crate::runtime::compiled_backends();
    for backend in KNOWN_RUNTIME_BACKENDS {
        let compiled_flag = if compiled_runtime.iter().any(|&b| b == backend) {
            "true"
        } else {
            "false"
        };
        if let Ok(gauge) = RUNTIME_BACKEND_INFO.handle_for_label_values(&[backend, compiled_flag]) {
            push_metric(
                &mut metrics,
                "runtime_backend_info",
                &[("backend", backend), ("compiled", compiled_flag)],
                gauge.get() as f64,
            );
        }
    }

    let compiled_transport = compiled_transport_providers();
    for provider in KNOWN_TRANSPORT_PROVIDERS {
        let compiled_flag = if compiled_transport.iter().any(|&p| p == provider) {
            "true"
        } else {
            "false"
        };
        if let Ok(gauge) =
            TRANSPORT_PROVIDER_INFO.handle_for_label_values(&[provider, compiled_flag])
        {
            push_metric(
                &mut metrics,
                "transport_provider_info",
                &[("provider", provider), ("compiled", compiled_flag)],
                gauge.get() as f64,
            );
        }
        if let Ok(counter) = TRANSPORT_PROVIDER_CONNECT_TOTAL.handle_for_label_values(&[provider]) {
            push_metric(
                &mut metrics,
                "transport_provider_connect_total",
                &[("provider", provider)],
                counter.get() as f64,
            );
        }
    }

    if let Ok(guard) = CODING_PREVIOUS.lock() {
        for (component, algorithm) in guard.iter() {
            for mode in ["active", "fallback", "emergency"] {
                if let Ok(gauge) = CODING_ALGORITHM_INFO.handle_for_label_values(&[
                    component,
                    algorithm.as_str(),
                    mode,
                ]) {
                    push_metric(
                        &mut metrics,
                        "coding_algorithm_info",
                        &[
                            ("component", *component),
                            ("algorithm", algorithm.as_str()),
                            ("mode", mode),
                        ],
                        gauge.get() as f64,
                    );
                }
            }
        }
    }

    for (codec_name, profiles) in KNOWN_CODEC_PROFILES {
        for profile in *profiles {
            if let Ok(counter) = CODEC_SERIALIZE_FAIL_TOTAL.handle_for_label_values(&[
                codec_name,
                profile,
                codec::VERSION,
            ]) {
                push_metric(
                    &mut metrics,
                    "codec_serialize_fail_total",
                    &[
                        ("codec", *codec_name),
                        ("profile", *profile),
                        ("version", codec::VERSION),
                    ],
                    counter.get() as f64,
                );
            }
            if let Ok(counter) = CODEC_DESERIALIZE_FAIL_TOTAL.handle_for_label_values(&[
                codec_name,
                profile,
                codec::VERSION,
            ]) {
                push_metric(
                    &mut metrics,
                    "codec_deserialize_fail_total",
                    &[
                        ("codec", *codec_name),
                        ("profile", *profile),
                        ("version", codec::VERSION),
                    ],
                    counter.get() as f64,
                );
            }
        }
    }

    for operation in ["sign", "verify", "verify_strict"] {
        for result in ["ok", "error"] {
            if let Ok(counter) = CRYPTO_OPERATION_TOTAL.handle_for_label_values(&[
                ed25519::ALGORITHM,
                ed25519::BACKEND,
                ed25519::BACKEND_VERSION,
                operation,
                result,
            ]) {
                push_metric(
                    &mut metrics,
                    "crypto_operation_total",
                    &[
                        ("algorithm", ed25519::ALGORITHM),
                        ("backend", ed25519::BACKEND),
                        ("version", ed25519::BACKEND_VERSION),
                        ("operation", operation),
                        ("result", result),
                    ],
                    counter.get() as f64,
                );
            }
        }
    }

    if let Ok(gauge) = CRYPTO_BACKEND_INFO.handle_for_label_values(&[
        ed25519::ALGORITHM,
        ed25519::BACKEND,
        ed25519::BACKEND_VERSION,
    ]) {
        push_metric(
            &mut metrics,
            "crypto_backend_info",
            &[
                ("algorithm", ed25519::ALGORITHM),
                ("backend", ed25519::BACKEND),
                ("version", ed25519::BACKEND_VERSION),
            ],
            gauge.get() as f64,
        );
    }

    WrapperSummary { metrics }
}

#[cfg(not(feature = "telemetry"))]
pub fn wrapper_metrics_snapshot() -> WrapperSummary {
    WrapperSummary::default()
}

#[cfg(feature = "telemetry")]
pub fn record_compression_ratio(algorithm: &str, ratio: f64) {
    STORAGE_COMPRESSION_RATIO
        .ensure_handle_for_label_values(&[algorithm])
        .expect(LABEL_REGISTRATION_ERR)
        .observe(ratio);
}

#[cfg(feature = "telemetry")]
pub fn set_sample_rate(rate: f64) {
    Lazy::force(&ADAPTIVE_LOOP);
    let scaled = (rate.clamp(0.0, 1.0) * 1_000_000.0) as u64;
    BASE_SAMPLE_RATE.store(scaled, Ordering::Relaxed);
    SAMPLE_RATE.store(scaled, Ordering::Relaxed);
}

#[cfg(feature = "telemetry")]
fn compact_histograms() {
    READ_STATS.reset_memory();
    update_memory_usage(MemoryComponent::Storage);
}

#[cfg(feature = "telemetry")]
pub fn set_compaction_interval(secs: u64) {
    COMPACTION_SECS.store(secs.max(1), Ordering::Relaxed);
    COMPACTOR.call_once(|| {
        std::thread::spawn(|| loop {
            let interval = COMPACTION_SECS.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(interval));
            compact_histograms();
        });
    });
    update_memory_usage(MemoryComponent::Storage);
}

#[cfg(feature = "telemetry")]
pub fn force_compact() {
    compact_histograms();
}

#[cfg(feature = "telemetry")]
pub fn current_alloc_bytes() -> u64 {
    process::resident_memory_bytes().unwrap_or(0)
}

#[cfg(feature = "telemetry")]
pub fn record_memory_bytes(component: MemoryComponent, bytes: u64) {
    TELEMETRY_ALLOC_BYTES.set(bytes as i64);
    READ_STATS.record_memory(component, bytes);
}

#[cfg(not(feature = "telemetry"))]
pub fn record_memory_bytes(_component: MemoryComponent, _bytes: u64) {}

#[cfg(feature = "telemetry")]
pub fn update_memory_usage(component: MemoryComponent) {
    let bytes = current_alloc_bytes();
    record_memory_bytes(component, bytes);
}

#[cfg(not(feature = "telemetry"))]
pub fn update_memory_usage(_component: MemoryComponent) {}

#[cfg(feature = "telemetry")]
fn adaptive_sampling_loop() {
    let mut last_rate = SAMPLE_RATE.load(Ordering::Relaxed);
    loop {
        std::thread::sleep(Duration::from_secs(ADAPTIVE_WINDOW_SECS));
        let base = BASE_SAMPLE_RATE.load(Ordering::Relaxed);
        if base == 0 {
            SAMPLE_RATE.store(0, Ordering::Relaxed);
            last_rate = 0;
            SAMPLE_FAIL_EVENTS.store(0, Ordering::Relaxed);
            continue;
        }
        let fails = SAMPLE_FAIL_EVENTS.swap(0, Ordering::Relaxed);
        let mut current = SAMPLE_RATE.load(Ordering::Relaxed);
        let floor = if base < ADAPTIVE_MIN_SAMPLE_RATE {
            base
        } else {
            ADAPTIVE_MIN_SAMPLE_RATE
        };

        let mut changed = false;
        if fails > 0 {
            let severity = (fails.min(10) as f64) * 0.05;
            let factor = (1.0 - severity).clamp(0.1, 1.0);
            let mut target = ((current as f64) * factor).round() as u64;
            if target < floor && base != 0 {
                target = floor;
            }
            if target < current {
                SAMPLE_RATE.store(target, Ordering::Relaxed);
                current = target;
                changed = true;
                diagnostics::tracing::warn!(
                    sample_rate_ppm = current,
                    fails,
                    "adaptive_sampling_throttled"
                );
            }
        } else if current < base {
            let diff = base - current;
            if diff > 0 {
                let step = ((diff as f64) * 0.25).ceil() as u64;
                let target = (current + step).min(base);
                if target != current {
                    SAMPLE_RATE.store(target, Ordering::Relaxed);
                    current = target;
                    changed = true;
                    diagnostics::tracing::info!(
                        sample_rate_ppm = current,
                        "adaptive_sampling_recovering"
                    );
                }
            }
        }

        if !changed && current != last_rate {
            diagnostics::tracing::debug!(sample_rate_ppm = current, "adaptive_sampling_updated");
        }
        last_rate = current;
    }
}

#[cfg(feature = "telemetry")]
pub fn sample_rate_ppm() -> u64 {
    SAMPLE_RATE.load(Ordering::Relaxed)
}

#[cfg(feature = "telemetry")]
pub fn compaction_interval_secs() -> u64 {
    COMPACTION_SECS.load(Ordering::Relaxed)
}

#[cfg(feature = "telemetry")]
pub fn record_log_correlation_fail() {
    LOG_CORRELATION_FAIL_TOTAL.inc();
    SAMPLE_FAIL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

#[cfg(not(feature = "telemetry"))]
pub fn sample_rate_ppm() -> u64 {
    0
}

#[cfg(not(feature = "telemetry"))]
pub fn compaction_interval_secs() -> u64 {
    0
}

#[cfg(not(feature = "telemetry"))]
pub fn record_log_correlation_fail() {}

#[cfg(not(feature = "telemetry"))]
pub fn sampled_inc(_c: &IntCounterHandle) {}
#[cfg(not(feature = "telemetry"))]
pub fn sampled_inc_vec(_c: &IntCounterVec, _l: &[&str]) {}
#[cfg(not(feature = "telemetry"))]
pub fn sampled_observe(_h: &HistogramHandle, _v: f64) {}
#[cfg(not(feature = "telemetry"))]
pub fn sampled_observe_vec(_h: &HistogramVec, _l: &[&str], _v: f64) {}

#[cfg(not(feature = "telemetry"))]
mod coding_stubs {
    pub fn record_coding_result(_stage: &str, _algorithm: &str, _result: &str) {}
    pub fn record_compression_ratio(_algorithm: &str, _ratio: f64) {}
}

#[cfg(not(feature = "telemetry"))]
pub use coding_stubs::{record_coding_result, record_compression_ratio};
#[cfg(not(feature = "telemetry"))]
pub fn set_sample_rate(_r: f64) {}
#[cfg(not(feature = "telemetry"))]
pub fn set_compaction_interval(_s: u64) {}
#[cfg(not(feature = "telemetry"))]
pub fn force_compact() {}
#[cfg(not(feature = "telemetry"))]
pub fn current_alloc_bytes() -> u64 {
    0
}
#[cfg(not(feature = "telemetry"))]
pub fn update_memory_usage(_component: MemoryComponent) {}

#[cfg(feature = "telemetry")]
pub fn log_context() -> diagnostics::tracing::Span {
    use rand::RngCore;
    let mut buf = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut buf);
    crate::logging::info_span_with_field("trace", "trace_id", crypto_suite::hex::encode(buf))
}

#[cfg(not(feature = "telemetry"))]
pub fn log_context() -> diagnostics::tracing::Span {
    diagnostics::tracing::Span::new(
        std::borrow::Cow::Borrowed("trace"),
        diagnostics::tracing::Level::INFO,
        Vec::new(),
    )
}

pub static HAAR_ETA_MILLI: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("haar_eta_milli", "eta parameter for burst veto x1000").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static UTIL_VAR_THRESHOLD_MILLI: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "util_var_threshold_milli",
        "utilisation variance threshold x1000",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static FIB_WINDOW_BASE_SECS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "fib_window_base_secs",
        "base seconds for Fibonacci smoothing",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static HEURISTIC_MU_MILLI: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("heuristic_mu_milli", "A* heuristic mu x1000").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static ACTIVE_MINERS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("active_miners", "effective active miners").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static BASE_FEE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("base_fee", "current base fee").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static BASE_REWARD_CT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("base_reward_ct", "base reward after logistic factor").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static MEMPOOL_SIZE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(Opts::new("mempool_size", "Current mempool size"), &["lane"])
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.ensure_handle_for_label_values(&["consumer"])
        .expect(LABEL_REGISTRATION_ERR)
        .set(0);
    g.ensure_handle_for_label_values(&["industrial"])
        .expect(LABEL_REGISTRATION_ERR)
        .set(0);
    g
});

pub static MEMPOOL_EVICTIONS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "mempool_evictions_total",
        "Total transactions evicted from the mempool",
    )
    .unwrap_or_else(|e| panic!("counter mempool evictions: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mempool evictions: {e}"));
    c.handle()
});

pub static FEE_FLOOR_CURRENT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "fee_floor_current",
        "Current dynamically computed fee floor",
    )
    .unwrap_or_else(|e| panic!("gauge fee floor current: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry fee floor current: {e}"));
    g.handle()
});

pub static FEE_FLOOR_WINDOW_CHANGED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "fee_floor_window_changed_total",
        "Total governance-triggered fee floor policy reconfigurations",
    )
    .unwrap_or_else(|e| panic!("counter fee floor window changed: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry fee floor window changed: {e}"));
    c.handle()
});

pub static FEE_FLOOR_WARNING_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "fee_floor_warning_total",
            "Wallet fee floor warnings surfaced to users",
        ),
        &["lane"],
    )
    .unwrap_or_else(|e| panic!("counter fee floor warning: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry fee floor warning: {e}"));
    c.ensure_handle_for_label_values(&["consumer"])
        .expect(LABEL_REGISTRATION_ERR)
        .inc_by(0);
    c.ensure_handle_for_label_values(&["industrial"])
        .expect(LABEL_REGISTRATION_ERR)
        .inc_by(0);
    c
});

pub static FEE_FLOOR_OVERRIDE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "fee_floor_override_total",
            "Wallet transactions forced below the advertised fee floor",
        ),
        &["lane"],
    )
    .unwrap_or_else(|e| panic!("counter fee floor override: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry fee floor override: {e}"));
    c.ensure_handle_for_label_values(&["consumer"])
        .expect(LABEL_REGISTRATION_ERR)
        .inc_by(0);
    c.ensure_handle_for_label_values(&["industrial"])
        .expect(LABEL_REGISTRATION_ERR)
        .inc_by(0);
    c
});

pub static DID_ANCHOR_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("did_anchor_total", "Total number of anchored DID documents")
        .unwrap_or_else(|e| panic!("counter did anchor: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry did anchor: {e}"));
    c.handle()
});

pub static PROOF_REBATES_CLAIMED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("proof_rebates_claimed_total", "Total proof rebate claims")
        .unwrap_or_else(|e| panic!("counter proof rebates claimed: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry proof rebates claimed: {e}"));
    c.handle()
});

pub static PROOF_REBATES_AMOUNT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "proof_rebates_amount_total",
        "Total CT awarded via proof rebates",
    )
    .unwrap_or_else(|e| panic!("counter proof rebates amount: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry proof rebates amount: {e}"));
    c.handle()
});

pub static PROOF_REBATES_PENDING_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "proof_rebates_pending_total",
        "Pending CT rebates awaiting claim",
    )
    .unwrap_or_else(|e| panic!("gauge proof rebates pending: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry proof rebates pending: {e}"));
    g.handle()
});

pub static MOBILE_CACHE_HIT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("mobile_cache_hit_total", "Total mobile cache hits")
        .unwrap_or_else(|e| panic!("counter mobile cache hit: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache hit: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_MISS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("mobile_cache_miss_total", "Total mobile cache misses")
        .unwrap_or_else(|e| panic!("counter mobile cache miss: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache miss: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_EVICT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "mobile_cache_evict_total",
        "Expired or purged mobile cache entries",
    )
    .unwrap_or_else(|e| panic!("counter mobile cache evict: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache evict: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_STALE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "mobile_cache_stale_total",
        "Mobile cache entries dropped due to TTL expiry",
    )
    .unwrap_or_else(|e| panic!("counter mobile cache stale: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache stale: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "mobile_cache_reject_total",
        "Mobile cache insertions rejected by limits",
    )
    .unwrap_or_else(|e| panic!("counter mobile cache reject: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache reject: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_ENTRY_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("mobile_cache_entry_total", "Active mobile cache entries")
        .unwrap_or_else(|e| panic!("gauge mobile cache entry total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache entry total: {e}"));
    g.handle()
});

pub static MOBILE_CACHE_ENTRY_BYTES: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "mobile_cache_entry_bytes",
        "Total bytes stored in the mobile cache",
    )
    .unwrap_or_else(|e| panic!("gauge mobile cache entry bytes: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache entry bytes: {e}"));
    g.handle()
});

pub static MOBILE_CACHE_QUEUE_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "mobile_cache_queue_total",
        "Offline transactions queued for replay",
    )
    .unwrap_or_else(|e| panic!("gauge mobile cache queue total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache queue total: {e}"));
    g.handle()
});

pub static MOBILE_CACHE_QUEUE_BYTES: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "mobile_cache_queue_bytes",
        "Bytes buffered in the mobile offline queue",
    )
    .unwrap_or_else(|e| panic!("gauge mobile cache queue bytes: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache queue bytes: {e}"));
    g.handle()
});

pub static MOBILE_CACHE_SWEEP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "mobile_cache_sweep_total",
        "Number of mobile cache TTL sweeps",
    )
    .unwrap_or_else(|e| panic!("counter mobile cache sweep: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache sweep: {e}"));
    c.handle()
});

pub static MOBILE_CACHE_SWEEP_WINDOW_SECONDS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "mobile_cache_sweep_window_seconds",
        "Configured sweep interval for the mobile cache",
    )
    .unwrap_or_else(|e| panic!("gauge mobile cache sweep window: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile cache sweep window: {e}"));
    g.handle()
});

pub static MOBILE_TX_QUEUE_DEPTH: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "mobile_tx_queue_depth",
        "Queued mobile transactions awaiting send",
    )
    .unwrap_or_else(|e| panic!("gauge mobile tx queue depth: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry mobile tx queue depth: {e}"));
    g.handle()
});

pub static SNAPSHOT_CREATED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("snapshot_created_total", "Total snapshots created")
        .unwrap_or_else(|e| panic!("counter snapshot created: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot created: {e}"));
    c.handle()
});

pub static SNAPSHOT_RESTORE_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("snapshot_restore_fail_total", "Failed snapshot restores")
        .unwrap_or_else(|e| panic!("counter snapshot restore fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot restore fail: {e}"));
    c.handle()
});

pub static SNAPSHOT_INTERVAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("snapshot_interval", "Snapshot interval in blocks")
        .unwrap_or_else(|e| panic!("gauge snapshot interval: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot interval: {e}"));
    g.handle()
});

pub static SNAPSHOT_INTERVAL_CHANGED: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "snapshot_interval_changed",
        "Last requested snapshot interval",
    )
    .unwrap_or_else(|e| panic!("gauge snapshot interval changed: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot interval changed: {e}"));
    g.handle()
});

pub static SNAPSHOT_DURATION_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new("snapshot_duration_seconds", "Snapshot operation duration");
    let h =
        Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram snapshot duration: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot duration: {e}"));
    h.handle()
});

pub static SNAPSHOT_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("snapshot_fail_total", "Total snapshot operation failures")
        .unwrap_or_else(|e| panic!("counter snapshot fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot fail: {e}"));
    c.handle()
});

pub static SUBSIDY_MULTIPLIER: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("subsidy_multiplier", "Current subsidy multipliers"),
        &["type"],
    )
    .unwrap_or_else(|e| panic!("gauge subsidy multiplier: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry subsidy multiplier: {e}"));
    g
});

pub static SUBSIDY_MULTIPLIER_RAW: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("subsidy_multiplier_raw", "True subsidy multipliers"),
        &["type"],
    )
    .unwrap_or_else(|e| panic!("gauge subsidy multiplier raw: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry subsidy multiplier raw: {e}"));
    g
});

pub static TELEMETRY_ALLOC_BYTES: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "telemetry_alloc_bytes",
        "Telemetry memory allocation in bytes",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static RUNTIME_BACKEND_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "runtime_backend_info",
            "Active async runtime backend (1 active / 0 inactive)",
        ),
        &["backend", "compiled"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec runtime_backend_info: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry runtime_backend_info: {e}"));
    g
});

pub static TRANSPORT_PROVIDER_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "transport_provider_info",
            "Transport provider selection (1 active / 0 inactive)",
        ),
        &["provider", "compiled"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec transport_provider_info: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry transport_provider_info: {e}"));
    g
});

pub static TRANSPORT_PROVIDER_CONNECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "transport_provider_connect_total",
            "Successful dial attempts grouped by transport provider",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("counter_vec transport_provider_connect_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry transport_provider_connect_total: {e}"));
    c
});

pub static CODING_ALGORITHM_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "coding_algorithm_info",
            "Coding component algorithm selection and fallback state",
        ),
        &["component", "algorithm", "mode"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec coding_algorithm_info: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry coding_algorithm_info: {e}"));
    g
});

pub static CODEC_PAYLOAD_BYTES: Lazy<HistogramVec> = Lazy::new(|| {
    let buckets = vec![
        64.0,
        256.0,
        1024.0,
        4096.0,
        16_384.0,
        65_536.0,
        262_144.0,
        1_048_576.0,
    ];
    let opts = HistogramOpts::new(
        "codec_payload_bytes",
        "Serialized payload size grouped by codec profile",
    )
    .buckets(buckets);
    let h = HistogramVec::new(opts, &["codec", "direction", "profile", "version"])
        .unwrap_or_else(|e| panic!("histogram_vec codec_payload_bytes: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry codec_payload_bytes: {e}"));
    h
});

pub static CODEC_SERIALIZE_FAIL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "codec_serialize_fail_total",
            "Codec serialization failures grouped by profile",
        ),
        &["codec", "profile", "version"],
    )
    .unwrap_or_else(|e| panic!("counter_vec codec_serialize_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry codec_serialize_fail_total: {e}"));
    c
});

pub static CODEC_DESERIALIZE_FAIL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "codec_deserialize_fail_total",
            "Codec deserialization failures grouped by profile",
        ),
        &["codec", "profile", "version"],
    )
    .unwrap_or_else(|e| panic!("counter_vec codec_deserialize_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry codec_deserialize_fail_total: {e}"));
    c
});

pub static CRYPTO_BACKEND_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "crypto_backend_info",
            "Active cryptographic backends (1 active / 0 inactive)",
        ),
        &["algorithm", "backend", "version"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec crypto_backend_info: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry crypto_backend_info: {e}"));
    g
});

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;
    use codec::{BinaryProfile, JsonProfile};

    fn reset_wrapper_metrics() {
        for labels in [
            ["json", "serialize", CODEC_NONE_PROFILE, codec::VERSION],
            ["binary", "serialize", "canonical", codec::VERSION],
            ["binary", "deserialize", "canonical", codec::VERSION],
            ["binary", "serialize", "transaction", codec::VERSION],
            ["binary", "deserialize", "transaction", codec::VERSION],
        ] {
            CODEC_PAYLOAD_BYTES.remove_label_values(&labels);
        }
        CODEC_SERIALIZE_FAIL_TOTAL.reset();
        CODEC_DESERIALIZE_FAIL_TOTAL.reset();
        CRYPTO_OPERATION_TOTAL.reset();
        CRYPTO_BACKEND_INFO.reset();
        RUNTIME_BACKEND_INFO.reset();
        TRANSPORT_PROVIDER_INFO.reset();
        TRANSPORT_PROVIDER_CONNECT_TOTAL.reset();
        CODING_ALGORITHM_INFO.reset();
        CODING_PREVIOUS.lock().unwrap().clear();
    }

    fn metric_value(
        summary: &WrapperSummary,
        metric: &str,
        labels: &[(&str, &str)],
    ) -> Option<f64> {
        summary.metrics.iter().find_map(|sample| {
            if sample.metric == metric
                && labels
                    .iter()
                    .all(|(key, value)| sample.labels.get(*key).map(|s| s.as_str()) == Some(*value))
            {
                Some(sample.value)
            } else {
                None
            }
        })
    }

    #[test]
    fn codec_hook_records_success_and_failures() {
        reset_wrapper_metrics();

        codec_metrics_hook(
            Codec::Json(JsonProfile::Canonical),
            Direction::Serialize,
            Some(128),
        );
        let hist = CODEC_PAYLOAD_BYTES
            .ensure_handle_for_label_values(&[
                "json",
                "serialize",
                CODEC_NONE_PROFILE,
                codec::VERSION,
            ])
            .expect(LABEL_REGISTRATION_ERR);
        assert_eq!(hist.get_sample_count(), 1);

        codec_metrics_hook(
            Codec::Binary(BinaryProfile::Transaction),
            Direction::Serialize,
            None,
        );
        let serialize_fail = CODEC_SERIALIZE_FAIL_TOTAL
            .handle_for_label_values(&["binary", "transaction", codec::VERSION])
            .unwrap();
        assert_eq!(serialize_fail.get(), 1);

        codec_metrics_hook(
            Codec::Binary(BinaryProfile::Transaction),
            Direction::Deserialize,
            None,
        );
        let deserialize_fail = CODEC_DESERIALIZE_FAIL_TOTAL
            .handle_for_label_values(&["binary", "transaction", codec::VERSION])
            .unwrap();
        assert_eq!(deserialize_fail.get(), 1);
    }

    #[test]
    fn crypto_hook_records_operation_results() {
        reset_wrapper_metrics();

        crypto_metrics_hook(ed25519::ALGORITHM, "sign", ed25519::BACKEND, true);
        crypto_metrics_hook(ed25519::ALGORITHM, "verify", ed25519::BACKEND, false);
        record_crypto_backend();

        let ok_counter = CRYPTO_OPERATION_TOTAL
            .handle_for_label_values(&[
                ed25519::ALGORITHM,
                ed25519::BACKEND,
                ed25519::BACKEND_VERSION,
                "sign",
                "ok",
            ])
            .unwrap();
        assert_eq!(ok_counter.get(), 1);

        let error_counter = CRYPTO_OPERATION_TOTAL
            .handle_for_label_values(&[
                ed25519::ALGORITHM,
                ed25519::BACKEND,
                ed25519::BACKEND_VERSION,
                "verify",
                "error",
            ])
            .unwrap();
        assert_eq!(error_counter.get(), 1);

        let backend_gauge = CRYPTO_BACKEND_INFO
            .handle_for_label_values(&[
                ed25519::ALGORITHM,
                ed25519::BACKEND,
                ed25519::BACKEND_VERSION,
            ])
            .unwrap();
        assert_eq!(backend_gauge.get(), 1);
    }

    #[test]
    fn wrapper_snapshot_aggregates_wrapper_metrics() {
        reset_wrapper_metrics();

        record_runtime_backend("inhouse");
        record_transport_backend("quinn");
        TRANSPORT_PROVIDER_CONNECT_TOTAL
            .ensure_handle_for_label_values(&["quinn"])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
        record_coding_algorithms(&crate::storage::settings::algorithms());
        codec_metrics_hook(
            Codec::Binary(BinaryProfile::Transaction),
            Direction::Serialize,
            None,
        );
        crypto_metrics_hook(ed25519::ALGORITHM, "sign", ed25519::BACKEND, true);

        let summary = wrapper_metrics_snapshot();
        assert!(summary.metrics.len() > 3);

        let runtime_value =
            metric_value(&summary, "runtime_backend_info", &[("backend", "inhouse")]);
        assert_eq!(runtime_value, Some(1.0));

        let transport_connect = metric_value(
            &summary,
            "transport_provider_connect_total",
            &[("provider", "quinn")],
        )
        .unwrap();
        assert_eq!(transport_connect, 1.0);

        let codec_failure = metric_value(
            &summary,
            "codec_serialize_fail_total",
            &[("codec", "binary"), ("profile", "transaction")],
        )
        .unwrap();
        assert_eq!(codec_failure, 1.0);

        let crypto_counter = metric_value(
            &summary,
            "crypto_operation_total",
            &[("operation", "sign"), ("result", "ok")],
        )
        .unwrap();
        assert_eq!(crypto_counter, 1.0);

        let coding_active = summary.metrics.iter().any(|sample| {
            sample.metric == "coding_algorithm_info"
                && sample.labels.get("mode").map(|s| s.as_str()) == Some("active")
                && sample.value == 1.0
        });
        assert!(coding_active);
    }
}

pub static CRYPTO_OPERATION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "crypto_operation_total",
            "Cryptographic operations grouped by backend and result",
        ),
        &["algorithm", "backend", "version", "operation", "result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec crypto_operation_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry crypto_operation_total: {e}"));
    c
});

pub static INDUSTRIAL_BACKLOG: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("industrial_backlog", "Pending industrial compute slices").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static INDUSTRIAL_UTILIZATION: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "industrial_utilization",
        "Industrial compute utilisation percentage",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static INDUSTRIAL_MULTIPLIER: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "industrial_multiplier",
        "Current industrial subsidy multiplier",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static INDUSTRIAL_UNITS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "industrial_units_total",
        "Total normalized compute units processed",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c.handle()
});

pub static INDUSTRIAL_PRICE_PER_UNIT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("industrial_price_per_unit", "Latest price per compute unit").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static PRICE_WEIGHT_APPLIED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "price_weight_applied_total",
        "Total price entries adjusted by reputation weight",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c.handle()
});

pub static PARALLEL_EXECUTE_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let buckets = telemetry::exponential_buckets(0.001, 2.0, 12);
    let opts = HistogramOpts::new(
        "parallel_execute_seconds",
        "Elapsed wall-clock time for ParallelExecutor batches",
    )
    .buckets(buckets);
    let h = Histogram::with_opts(opts)
        .unwrap_or_else(|e| panic!("histogram parallel execute seconds: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry parallel execute seconds: {e}"));
    h.handle()
});

pub static DEX_ESCROW_LOCKED: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("dex_escrow_locked", "Total funds locked in DEX escrow").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static DEX_ESCROW_PENDING: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("dex_escrow_pending", "Number of pending DEX escrows").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static DEX_LIQUIDITY_LOCKED_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "dex_liquidity_locked_total",
        "Total liquidity currently locked in DEX escrow",
    )
    .unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g.handle()
});

pub static DEX_ORDERS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("dex_orders_total", "Orders submitted to the DEX by side"),
        &["side"],
    )
    .unwrap_or_else(|e| panic!("counter dex orders: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry dex orders: {e}"));
    c
});

pub static DEX_TRADE_VOLUME: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "dex_trade_volume",
        "Total matched trade quantity across all DEX pairs",
    )
    .unwrap_or_else(|e| panic!("counter dex trade volume: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry dex trade volume: {e}"));
    c.handle()
});

pub static TOKENS_CREATED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("tokens_created_total", "Total number of registered tokens")
        .unwrap_or_else(|e| panic!("counter tokens created: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry tokens created: {e}"));
    c.handle()
});

pub static TOKEN_BRIDGE_VOLUME_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "token_bridge_volume_total",
        "Volume bridged via token bridge",
    )
    .unwrap_or_else(|e| panic!("counter token bridge volume: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry token bridge volume: {e}"));
    c.handle()
});

pub static HTLC_CREATED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("htlc_created_total", "HTLC contracts created")
        .unwrap_or_else(|e| panic!("counter htlc created: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry htlc created: {e}"));
    c.handle()
});

pub static HTLC_REFUNDED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("htlc_refunded_total", "HTLC contracts refunded")
        .unwrap_or_else(|e| panic!("counter htlc refunded: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry htlc refunded: {e}"));
    c.handle()
});

pub static TX_BY_JURISDICTION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "tx_by_jurisdiction_total",
            "Transactions processed per jurisdiction tag",
        ),
        &["jurisdiction"],
    )
    .unwrap_or_else(|e| panic!("counter tx jurisdiction: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry tx jurisdiction: {e}"));
    c
});

pub static STATE_STREAM_SUBSCRIBERS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "state_stream_subscribers_total",
        "Total websocket state stream subscribers",
    )
    .unwrap_or_else(|e| panic!("counter state stream subs: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry state stream subs: {e}"));
    c.handle()
});

pub static STATE_STREAM_LAG_ALERT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "state_stream_lag_alert_total",
        "Clients falling behind alert count",
    )
    .unwrap_or_else(|e| panic!("counter state stream lag: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry state stream lag: {e}"));
    c.handle()
});

pub static VM_TRACE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("vm_trace_total", "Total VM trace sessions")
        .unwrap_or_else(|e| panic!("counter vm trace: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry vm trace: {e}"));
    c.handle()
});

pub static SUBSIDY_BYTES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("subsidy_bytes_total", "Total subsidized bytes by type"),
        &["type"],
    )
    .unwrap_or_else(|e| panic!("counter subsidy bytes: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry subsidy bytes: {e}"));
    c
});

pub static SUBSIDY_CPU_MS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "subsidy_cpu_ms_total",
        "Total subsidized compute time in ms",
    )
    .unwrap_or_else(|e| panic!("counter subsidy cpu: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry subsidy cpu: {e}"));
    c.handle()
});

pub static VM_GAS_USED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("vm_gas_used_total", "Total gas consumed by VM executions")
        .unwrap_or_else(|e| panic!("counter vm gas used: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry vm gas used: {e}"));
    c.handle()
});

pub static WASM_CONTRACT_EXECUTIONS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "wasm_contract_executions_total",
        "Total WASM contract executions",
    )
    .unwrap_or_else(|e| panic!("counter wasm exec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry wasm exec: {e}"));
    c.handle()
});

pub static WASM_GAS_CONSUMED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "wasm_gas_consumed_total",
        "Total gas used by WASM contracts",
    )
    .unwrap_or_else(|e| panic!("counter wasm gas: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry wasm gas: {e}"));
    c.handle()
});

pub static VM_OUT_OF_GAS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("vm_out_of_gas_total", "VM executions that ran out of gas")
        .unwrap_or_else(|e| panic!("counter vm out of gas: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry vm out of gas: {e}"));
    c.handle()
});

pub static BADGE_ISSUED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("badge_issued_total", "Service badges issued")
        .unwrap_or_else(|e| panic!("counter badge issued: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry badge issued: {e}"));
    c.handle()
});

pub static BADGE_REVOKED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("badge_revoked_total", "Service badges revoked")
        .unwrap_or_else(|e| panic!("counter badge revoked: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry badge revoked: {e}"));
    c.handle()
});

pub static ANOMALY_LABEL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "anomaly_labels_total",
        "Anomaly labels submitted for model feedback",
    )
    .unwrap_or_else(|e| panic!("counter anomaly labels: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry anomaly labels: {e}"));
    c.handle()
});

pub static DKG_ROUND_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("dkg_round_total", "Completed DKG rounds")
        .unwrap_or_else(|e| panic!("counter dkg round: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry dkg round: {e}"));
    c.handle()
});

pub static THRESHOLD_SIGNATURE_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "threshold_signature_fail_total",
        "Failed threshold signature verifications",
    )
    .unwrap_or_else(|e| panic!("counter threshold sig fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry threshold sig fail: {e}"));
    c.handle()
});

#[cfg(feature = "telemetry")]
pub fn export_dataset<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::Write;
    let metric_families = REGISTRY.gather();
    let mut buf = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buf)
        .unwrap();
    File::create(path)?.write_all(&buf)?;
    Ok(())
}

pub static SUBSIDY_AUTO_REDUCED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "subsidy_auto_reduced_total",
        "Multiplier auto-reduction events due to inflation guard",
    )
    .unwrap_or_else(|e| panic!("counter subsidy auto reduced: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry subsidy auto reduced: {e}"));
    c.handle()
});

pub static KILL_SWITCH_TRIGGER_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "kill_switch_trigger_total",
        "Times the subsidy kill switch was activated",
    )
    .unwrap_or_else(|e| panic!("counter kill switch trigger: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry kill switch trigger: {e}"));
    c.handle()
});

pub static MINER_REWARD_RECALC_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "miner_reward_recalc_total",
        "Times the miner reward logistic factor was recalculated",
    )
    .unwrap_or_else(|e| panic!("counter miner reward recalc: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry miner reward recalc: {e}"));
    c.handle()
});

pub static DIFFICULTY_RETARGET_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "difficulty_retarget_total",
        "Number of difficulty retarget calculations",
    )
    .unwrap_or_else(|e| panic!("counter difficulty retarget: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry difficulty retarget: {e}"));
    c.handle()
});

pub static DIFFICULTY_CLAMP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "difficulty_clamp_total",
        "Retarget calculations clamped to bounds",
    )
    .unwrap_or_else(|e| panic!("counter difficulty clamp: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry difficulty clamp: {e}"));
    c.handle()
});

pub static DIFFICULTY_WINDOW_SHORT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "difficulty_window_short",
        "Short-term EMA of block intervals in ms",
    )
    .unwrap_or_else(|e| panic!("gauge difficulty window short: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry difficulty window short: {e}"));
    g.handle()
});

pub static DIFFICULTY_WINDOW_MED: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "difficulty_window_med",
        "Medium-term EMA of block intervals in ms",
    )
    .unwrap_or_else(|e| panic!("gauge difficulty window med: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry difficulty window med: {e}"));
    g.handle()
});

pub static DIFFICULTY_WINDOW_LONG: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "difficulty_window_long",
        "Long-term EMA of block intervals in ms",
    )
    .unwrap_or_else(|e| panic!("gauge difficulty window long: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry difficulty window long: {e}"));
    g.handle()
});

pub static RENT_ESCROW_LOCKED_CT_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "rent_escrow_locked_ct_total",
        "Total CT locked in rent escrow",
    )
    .unwrap_or_else(|e| panic!("gauge rent escrow: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry rent escrow: {e}"));
    g.handle()
});

pub static RENT_ESCROW_REFUNDED_CT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "rent_escrow_refunded_ct_total",
        "Total CT refunded from rent escrow",
    )
    .unwrap_or_else(|e| panic!("counter rent escrow refunded: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry rent escrow refunded: {e}"));
    c.handle()
});

pub static RENT_ESCROW_BURNED_CT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "rent_escrow_burned_ct_total",
        "Total CT burned from rent escrow",
    )
    .unwrap_or_else(|e| panic!("counter rent escrow burned: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry rent escrow burned: {e}"));
    c.handle()
});

pub static SLASHING_BURN_CT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "slashing_burn_ct_total",
        "Total CT burned from slashing penalties",
    )
    .unwrap_or_else(|e| panic!("counter slashing burn: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry slashing burn: {e}"));
    c.handle()
});

pub static EVICTIONS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("evictions_total", "Total mempool evictions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static SHARD_CACHE_EVICT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("shard_cache_evict_total", "Total shard cache evictions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static INTER_SHARD_REPLAY_EVICT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "inter_shard_replay_evict_total",
        "Total inter-shard replay cache evictions",
    )
    .unwrap_or_else(|e| panic!("counter inter_shard_replay_evict_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry inter_shard_replay_evict_total: {e}"));
    c.handle()
});

pub static FEE_FLOOR_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "fee_floor_reject_total",
        "Transactions rejected for low fee",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static INDUSTRIAL_ADMITTED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "industrial_admitted_total",
        "Industrial lane transactions admitted",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static INDUSTRIAL_DEFERRED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "industrial_deferred_total",
        "Industrial lane submissions deferred",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static INDUSTRIAL_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "industrial_rejected_total",
            "Industrial admission rejections by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PAYOUT_CAP_HITS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "payout_cap_hits_total",
            "Number of settlement payouts capped per identity",
        ),
        &["identity"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static ACTIVE_BURST_QUOTA: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("active_burst_quota", "Remaining burst quota"),
        &["identity"],
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static PARTITION_EVENTS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "partition_events_total",
        "Number of detected network partitions",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static PARTITION_RECOVER_BLOCKS: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "partition_recover_blocks",
        "Blocks replayed during partition recovery",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static ADMISSION_MODE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("admission_mode", "Current industrial admission mode"),
        &["mode"],
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static PARAM_CHANGE_PENDING: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "param_change_pending",
            "Governance parameter changes pending activation",
        ),
        &["key"],
    )
    .unwrap_or_else(|e| panic!("gauge param_change_pending: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry param_change_pending: {e}"));
    g
});

pub static PARAM_CHANGE_ACTIVE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "param_change_active",
            "Current active governance parameter values",
        ),
        &["key"],
    )
    .unwrap_or_else(|e| panic!("gauge param_change_active: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry param_change_active: {e}"));
    g
});

pub static CONSUMER_FEE_P50: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("consumer_fee_p50", "Median consumer fee")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static CONSUMER_FEE_P90: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("consumer_fee_p90", "p90 consumer fee")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static READ_DENIED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("read_denied_total", "Reads denied by reason"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter read denied: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry read denied: {e}"));
    c
});

pub static READ_ACK_PROCESSED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "read_ack_processed_total",
            "Gateway read acknowledgements processed by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter read ack processed: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry read ack processed: {e}"));
    c
});

pub static READ_SELECTION_PROOF_VERIFIED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "read_selection_proof_verified_total",
            "Validated selection proofs by attestation type",
        ),
        &["attestation"],
    )
    .unwrap_or_else(|e| panic!("counter read selection proof verified: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry read selection proof verified: {e}"));
    c
});

pub static READ_SELECTION_PROOF_INVALID_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "read_selection_proof_invalid_total",
            "Rejected selection proofs by attestation type",
        ),
        &["attestation"],
    )
    .unwrap_or_else(|e| panic!("counter read selection proof invalid: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry read selection proof invalid: {e}"));
    c
});

pub static READ_SELECTION_PROOF_LATENCY_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let buckets = telemetry::exponential_buckets(0.001, 2.0, 16);
    let opts = HistogramOpts::new(
        "read_selection_proof_latency_seconds",
        "Selection proof verification latency by attestation",
    )
    .buckets(buckets);
    let hv = HistogramVec::new(opts, &["attestation"])
        .unwrap_or_else(|e| panic!("histogram read selection proof latency: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry read selection proof latency: {e}"));
    hv
});

pub static AD_VERIFIER_COMMITTEE_REJECTION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "ad_verifier_committee_rejection_total",
            "Ad verifier committee attestation rejections by committee and reason",
        ),
        &["committee", "reason"],
    )
    .unwrap_or_else(|e| panic!("counter ad verifier committee rejection: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry ad verifier committee rejection: {e}"));
    c
});

#[cfg(feature = "telemetry")]
pub fn reset_ad_verifier_committee_rejections() {
    AD_VERIFIER_COMMITTEE_REJECTION_TOTAL.reset();
}

#[cfg(not(feature = "telemetry"))]
pub fn reset_ad_verifier_committee_rejections() {}

#[cfg(feature = "telemetry")]
pub fn ensure_ad_verifier_committee_label(committee: &str, reason: &str) {
    AD_VERIFIER_COMMITTEE_REJECTION_TOTAL
        .ensure_handle_for_label_values(&[committee, reason])
        .expect(LABEL_REGISTRATION_ERR);
}

#[cfg(not(feature = "telemetry"))]
pub fn ensure_ad_verifier_committee_label(_committee: &str, _reason: &str) {}

pub static AD_READINESS_SKIPPED: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "ad_readiness_skipped_total",
            "Ad impressions skipped due to readiness blockers",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter ad readiness skipped: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry ad readiness skipped: {e}"));
    c
});

#[cfg(feature = "telemetry")]
pub static AD_MARKET_UTILIZATION_OBSERVED: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "ad_market_utilization_observed_ppm",
            "Observed cohort utilization in parts-per-million",
        ),
        &["domain", "provider", "badges"],
    )
    .unwrap_or_else(|e| panic!("gauge ad market utilization observed: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad market utilization observed: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_MARKET_UTILIZATION_TARGET: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "ad_market_utilization_target_ppm",
            "Target cohort utilization in parts-per-million",
        ),
        &["domain", "provider", "badges"],
    )
    .unwrap_or_else(|e| panic!("gauge ad market utilization target: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad market utilization target: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_MARKET_UTILIZATION_DELTA: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "ad_market_utilization_delta_ppm",
            "Observed minus target utilization in parts-per-million",
        ),
        &["domain", "provider", "badges"],
    )
    .unwrap_or_else(|e| panic!("gauge ad market utilization delta: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad market utilization delta: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static AD_MARKET_UTILIZATION_LABELS: Lazy<Mutex<HashSet<(String, String, String)>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_CONFIG_VALUES: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_config_value",
            "Budget broker configuration parameters exposed for pacing inspection",
        ),
        &["parameter"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget config value: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_CAMPAIGN_REMAINING_USD: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_campaign_remaining_usd",
            "Remaining campaign budget tracked by the broker (USD micros)",
        ),
        &["campaign"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget campaign remaining: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_CAMPAIGN_DUAL_PRICE: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_campaign_dual_price",
            "Current dual price for campaign pacing",
        ),
        &["campaign"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget campaign dual price: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_CAMPAIGN_EPOCH_TARGET_USD: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_campaign_epoch_target_usd",
            "Per-epoch spend target for campaigns (USD micros)",
        ),
        &["campaign"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget campaign epoch target: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_COHORT_KAPPA: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_cohort_kappa",
            "Cohort-level pacing multiplier (kappa)",
        ),
        &["campaign", "domain", "provider", "badges"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget cohort kappa: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_COHORT_ERROR: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new("ad_budget_cohort_error", "Smoothed pacing error per cohort"),
        &["campaign", "domain", "provider", "badges"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget cohort error: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_COHORT_REALIZED_USD: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_cohort_realized_usd",
            "Realized spend per cohort (USD micros)",
        ),
        &["campaign", "domain", "provider", "badges"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget cohort realized: {e}"));
    g
});

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_SUMMARY_VALUES: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "ad_budget_summary_value",
            "Aggregated budget broker analytics for pacing diagnostics",
        ),
        &["metric"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget summary value: {e}"));
    g
});

#[cfg(feature = "telemetry")]
static AD_BUDGET_CAMPAIGN_LABELS: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

#[cfg(feature = "telemetry")]
static AD_BUDGET_COHORT_LABELS: Lazy<Mutex<HashSet<(String, String, String, String)>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

#[cfg(feature = "telemetry")]
pub static AD_BUDGET_SNAPSHOT_GENERATED_AT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let gauge = IntGauge::new(
        "ad_budget_snapshot_generated_at_micros",
        "Timestamp of the most recent budget snapshot in microseconds",
    )
    .unwrap_or_else(|e| panic!("gauge ad budget snapshot generated at: {e}"));
    REGISTRY
        .register(Box::new(gauge.clone()))
        .unwrap_or_else(|e| panic!("registry ad budget snapshot generated at: {e}"));
    gauge.handle()
});

pub static STORAGE_CHUNK_SIZE_BYTES: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "storage_chunk_size_bytes",
        "Size of chunks put into storage",
    );
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h.handle()
});

pub static STORAGE_CODING_OPERATIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "storage_coding_operations_total",
            "Storage coding operations by stage, algorithm, and result",
        ),
        &["stage", "algorithm", "result"],
    )
    .unwrap_or_else(|e| panic!("counter storage coding ops: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry storage coding ops: {e}"));
    c
});

pub static STORAGE_COMPRESSION_RATIO: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "storage_compression_ratio",
        "Compression ratios achieved per algorithm",
    );
    let hv = HistogramVec::new(opts, &["algorithm"])
        .unwrap_or_else(|e| panic!("histogram storage compression ratio: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry storage compression ratio: {e}"));
    hv
});

pub static STORAGE_PUT_OBJECT_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let buckets = telemetry::exponential_buckets(0.005, 1.8, 12);
    let opts = HistogramOpts::new(
        "storage_put_object_seconds",
        "End-to-end latency for StoragePipeline::put_object",
    )
    .buckets(buckets);
    let hv = HistogramVec::new(opts, &["erasure", "compression"])
        .unwrap_or_else(|e| panic!("histogram storage put object: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry storage put object: {e}"));
    hv
});

pub static STORAGE_PUT_CHUNK_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new("storage_put_chunk_seconds", "Time to put a single chunk");
    let hv = HistogramVec::new(opts, &["erasure", "compression"])
        .unwrap_or_else(|e| panic!("histogram storage put chunk: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry storage put chunk: {e}"));
    hv
});

pub static STORAGE_PROVIDER_RTT_MS: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "storage_provider_rtt_ms",
        "Observed provider RTT in milliseconds",
    );
    let hv = HistogramVec::new(opts, &["provider"]).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    hv
});

pub static STORAGE_PROVIDER_LOSS_RATE: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new("storage_provider_loss_rate", "Observed provider loss rate");
    let hv = HistogramVec::new(opts, &["provider"]).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    hv
});

pub static STORAGE_REPAIR_BYTES_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "storage_repair_bytes_total",
        "Total bytes reconstructed by repair loop",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static STORAGE_REPAIR_ATTEMPTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "storage_repair_attempts_total",
            "Storage repair attempts by outcome",
        ),
        &["status"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static STORAGE_REPAIR_FAILURES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "storage_repair_failures_total",
            "Total storage repair failures by error category",
        ),
        &["error", "erasure", "compression"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static STORAGE_INITIAL_CHUNK_SIZE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "storage_initial_chunk_size",
        "Initial chunk size used for object upload",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static STORAGE_FINAL_CHUNK_SIZE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "storage_final_chunk_size",
        "Final preferred chunk size after upload",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static STORAGE_PUT_ETA_SECONDS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "storage_put_eta_seconds",
        "Estimated time to upload object in seconds",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static STORAGE_DISK_FULL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "storage_disk_full_total",
        "Number of storage writes that failed due to disk exhaustion",
    )
    .unwrap_or_else(|e| panic!("counter storage_disk_full_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry storage_disk_full_total: {e}"));
    c.handle()
});

pub static STORAGE_COMPACTION_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "storage_compaction_total",
        "Number of RocksDB compaction operations",
    )
    .unwrap_or_else(|e| panic!("counter storage_compaction_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry storage_compaction_total: {e}"));
    c.handle()
});

pub static STORAGE_ENGINE_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_info",
            "Storage engine backend selection (1 for active backend)",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_info: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_info: {e}"));
    g
});

pub static STORAGE_ENGINE_PENDING_COMPACTIONS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_pending_compactions",
            "Pending compactions reported by the storage engine",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_pending_compactions: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_pending_compactions: {e}"));
    g
});

pub static STORAGE_ENGINE_RUNNING_COMPACTIONS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_running_compactions",
            "Active compactions reported by the storage engine",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_running_compactions: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_running_compactions: {e}"));
    g
});

pub static STORAGE_ENGINE_LEVEL0_FILES: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_level0_files",
            "Level-0 file count per storage engine",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_level0_files: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_level0_files: {e}"));
    g
});

pub static STORAGE_ENGINE_SST_BYTES: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_sst_bytes",
            "Total bytes stored in SST files",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_sst_bytes: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_sst_bytes: {e}"));
    g
});

pub static STORAGE_ENGINE_MEMTABLE_BYTES: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_memtable_bytes",
            "Bytes retained in storage engine memtables",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_memtable_bytes: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_memtable_bytes: {e}"));
    g
});

pub static STORAGE_ENGINE_SIZE_BYTES: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "storage_engine_size_bytes",
            "Bytes consumed on disk by the storage engine",
        ),
        &["db", "engine"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec storage_engine_size_bytes: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry storage_engine_size_bytes: {e}"));
    g
});

pub static STORAGE_CONTRACT_CREATED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "storage_contract_created_total",
        "Total number of storage contracts created",
    )
    .unwrap_or_else(|e| panic!("counter storage_contract_created_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry storage_contract_created_total: {e}"));
    c.handle()
});

pub static RETRIEVAL_FAILURE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "retrieval_failure_total",
        "Total failed proof-of-retrievability challenges",
    )
    .unwrap_or_else(|e| panic!("counter retrieval_failure_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry retrieval_failure_total: {e}"));
    c.handle()
});

pub static RETRIEVAL_SUCCESS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "retrieval_success_total",
        "Total successful proof-of-retrievability challenges",
    )
    .unwrap_or_else(|e| panic!("counter retrieval_success_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry retrieval_success_total: {e}"));
    c.handle()
});

pub static MATCHES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("matches_total", "Total matched jobs"),
        &["dry_run", "lane"],
    )
    .unwrap_or_else(|e| panic!("counter matches_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry matches_total: {e}"));
    c
});

pub static SNARK_VERIFICATIONS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "snark_verifications_total",
        "Successfully verified SNARK proofs",
    )
    .unwrap_or_else(|e| panic!("counter snark_verifications_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snark_verifications_total: {e}"));
    c.handle()
});

pub static SNARK_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("snark_fail_total", "Failed SNARK proof verifications")
        .unwrap_or_else(|e| panic!("counter snark_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snark_fail_total: {e}"));
    c.handle()
});

pub static SHIELDED_TX_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("shielded_tx_total", "Total shielded transactions accepted")
        .unwrap_or_else(|e| panic!("counter shielded_tx_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry shielded_tx_total: {e}"));
    c.handle()
});

pub static SHIELDED_POOL_SIZE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "shielded_pool_size",
        "Number of pending shielded nullifiers",
    )
    .unwrap_or_else(|e| panic!("gauge shielded_pool_size: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry shielded_pool_size: {e}"));
    g.handle()
});

pub static SCHEDULER_MATCH_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "scheduler_match_total",
            "Scheduler match outcomes by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter scheduler_match_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_match_total: {e}"));
    c
});

pub static SCHEDULER_MATCH_LATENCY_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "scheduler_match_latency_seconds",
        "Time to perform a scheduler match",
    );
    let h = Histogram::with_opts(opts)
        .unwrap_or_else(|e| panic!("histogram scheduler match latency: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler match latency: {e}"));
    h.handle()
});

pub static SCHEDULER_CLASS_WAIT_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "scheduler_class_wait_seconds",
        "Wait time per service class before execution",
    );
    let h = HistogramVec::new(opts, &["class"])
        .unwrap_or_else(|e| panic!("histogram vec scheduler class wait: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler class wait: {e}"));
    h
});

pub static SCHEDULER_REPUTATION_SCORE: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "scheduler_provider_reputation",
        "Distribution of provider reputation scores",
    );
    let h = Histogram::with_opts(opts)
        .unwrap_or_else(|e| panic!("histogram scheduler provider reputation: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler provider reputation: {e}"));
    h.handle()
});

pub static SCHEDULER_ACTIVE_JOBS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("scheduler_active_jobs", "Number of currently assigned jobs")
        .unwrap_or_else(|e| panic!("gauge scheduler_active_jobs: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_active_jobs: {e}"));
    g.handle()
});

pub static SCHEDULER_THREAD_COUNT: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "scheduler_thread_count",
        "Current compute scheduler worker threads",
    )
    .unwrap_or_else(|e| panic!("gauge scheduler_thread_count: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_thread_count: {e}"));
    g.handle()
});

pub static HASH_OPS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "hash_ops_total",
        "Total number of hash operations measured via perf counters",
    )
    .unwrap_or_else(|e| panic!("counter hash_ops_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry hash_ops_total: {e}"));
    c.handle()
});

pub static SIGVERIFY_OPS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "sigverify_ops_total",
        "Total number of signature verifications measured via perf counters",
    )
    .unwrap_or_else(|e| panic!("counter sigverify_ops_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry sigverify_ops_total: {e}"));
    c.handle()
});

pub static LIGHT_CLIENT_STREAM_OVERHEAD: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "light_client_stream_overhead_bytes_total",
        "Bytes of overhead for light-client streaming",
    )
    .unwrap_or_else(|e| panic!("counter light_client_stream_overhead_bytes_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry light_client_stream_overhead_bytes_total: {e}"));
    c.handle()
});

pub static STATE_SYNC_OVERHEAD: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "state_sync_overhead_bytes_total",
        "Bytes of overhead for state sync streaming",
    )
    .unwrap_or_else(|e| panic!("counter state_sync_overhead_bytes_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry state_sync_overhead_bytes_total: {e}"));
    c.handle()
});

pub static RPC_LATENCY: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new("rpc_latency_seconds", "Latency histogram per RPC module");
    let hv = HistogramVec::new(opts, &["module"])
        .unwrap_or_else(|e| panic!("histogram rpc_latency_seconds: {e}"));
    REGISTRY
        .register(Box::new(hv.clone()))
        .unwrap_or_else(|e| panic!("registry rpc_latency_seconds: {e}"));
    hv
});

pub static ANOMALY_ALARM_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "anomaly_alarm_total",
        "Total number of anomaly alarms raised",
    )
    .unwrap_or_else(|e| panic!("counter anomaly_alarm_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry anomaly_alarm_total: {e}"));
    c.handle()
});

pub fn trigger_anomaly(reason: &str) {
    #[cfg(feature = "telemetry")]
    {
        ANOMALY_ALARM_TOTAL.inc();
        diagnostics::tracing::warn!(reason, "anomaly_alarm");
    }
}

pub fn record_rpc_latency(module: &str, secs: f64) {
    #[cfg(feature = "telemetry")]
    RPC_LATENCY
        .ensure_handle_for_label_values(&[module])
        .expect(LABEL_REGISTRATION_ERR)
        .observe(secs);
}

pub fn rpc_latency_count(module: &str) -> u64 {
    #[cfg(feature = "telemetry")]
    {
        RPC_LATENCY
            .ensure_handle_for_label_values(&[module])
            .expect(LABEL_REGISTRATION_ERR)
            .get_sample_count()
    }
    #[cfg(not(feature = "telemetry"))]
    {
        0
    }
}

pub fn auto_tune() {
    #[cfg(feature = "telemetry")]
    {
        println!("running auto-profile harness");
        // Placeholder for real benchmarking logic
    }
}

pub static SCHEDULER_PRIORITY_MISS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "scheduler_priority_miss_total",
        "High-priority jobs exceeding wait threshold",
    )
    .unwrap_or_else(|e| panic!("counter scheduler_priority_miss_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_priority_miss_total: {e}"));
    c.handle()
});

pub static SCHEDULER_JOB_AGE_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let h = Histogram::with_opts(HistogramOpts::new(
        "job_age_seconds",
        "Time a job waited in the scheduler queue",
    ))
    .unwrap_or_else(|e| panic!("hist job_age_seconds: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry job_age_seconds: {e}"));
    h.handle()
});

pub static SCHEDULER_PRIORITY_BOOST_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "priority_boost_total",
        "Jobs whose priority was boosted due to aging",
    )
    .unwrap_or_else(|e| panic!("counter priority_boost_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry priority_boost_total: {e}"));
    c.handle()
});

pub static SCHEDULER_EFFECTIVE_PRICE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "scheduler_effective_price",
            "Effective compute price per unit by provider",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("gauge scheduler_effective_price: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_effective_price: {e}"));
    g
});

pub static SCHEDULER_PREEMPT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "scheduler_preempt_total",
            "Scheduler preemption events by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter scheduler_preempt_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_preempt_total: {e}"));
    c
});

pub static SCHEDULER_CANCEL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("scheduler_cancel_total", "Scheduler cancellations"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter scheduler_cancel_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_cancel_total: {e}"));
    c
});

pub static COMPUTE_JOB_TIMEOUT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "compute_job_timeout_total",
        "Jobs exceeding declared deadlines",
    )
    .unwrap_or_else(|e| panic!("counter compute_job_timeout_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry compute_job_timeout_total: {e}"));
    c.handle()
});

pub static JOB_RESUBMITTED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "job_resubmitted_total",
        "Jobs resubmitted after provider failure",
    )
    .unwrap_or_else(|e| panic!("counter job_resubmitted_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry job_resubmitted_total: {e}"));
    c.handle()
});

pub static COMPUTE_SLA_VIOLATIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "compute_sla_violations_total",
            "Total compute provider SLA violations",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("counter compute_sla_violations_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry compute_sla_violations_total: {e}"));
    c
});

pub static COMPUTE_SLA_PENDING_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "compute_sla_pending_total",
        "Number of compute jobs with active SLA tracking",
    )
    .unwrap_or_else(|e| panic!("gauge compute_sla_pending_total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry compute_sla_pending_total: {e}"));
    g.handle()
});

pub static COMPUTE_SLA_NEXT_DEADLINE_TS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "compute_sla_next_deadline_ts",
        "Unix timestamp of the next pending compute SLA deadline",
    )
    .unwrap_or_else(|e| panic!("gauge compute_sla_next_deadline_ts: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry compute_sla_next_deadline_ts: {e}"));
    g.handle()
});

pub static COMPUTE_SLA_AUTOMATED_SLASH_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "compute_sla_automated_slash_total",
        "Count of SLA penalties applied automatically by the settlement engine",
    )
    .unwrap_or_else(|e| panic!("counter compute_sla_automated_slash_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry compute_sla_automated_slash_total: {e}"));
    c.handle()
});

pub static COMPUTE_PROVIDER_UPTIME: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "compute_provider_uptime",
            "Rolling uptime percentage for the compute provider",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("gauge compute_provider_uptime: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry compute_provider_uptime: {e}"));
    g
});

pub static SCHEDULER_ACCELERATOR_MISS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "scheduler_accelerator_miss_total",
        "Jobs requiring accelerators that could not be matched",
    )
    .unwrap_or_else(|e| panic!("counter scheduler_accelerator_miss_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_accelerator_miss_total: {e}"));
    c.handle()
});

pub static SCHEDULER_ACCELERATOR_UTIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "scheduler_accelerator_util_total",
        "Jobs requiring accelerators that started successfully",
    )
    .unwrap_or_else(|e| panic!("counter scheduler_accelerator_util_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_accelerator_util_total: {e}"));
    c.handle()
});

pub static SCHEDULER_ACCELERATOR_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "scheduler_accelerator_fail_total",
        "Accelerator jobs that failed or were cancelled",
    )
    .unwrap_or_else(|e| panic!("counter scheduler_accelerator_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry scheduler_accelerator_fail_total: {e}"));
    c.handle()
});

pub static REPUTATION_ADJUST_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("reputation_adjust_total", "Reputation adjustments"),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter reputation_adjust_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry reputation_adjust_total: {e}"));
    c
});

pub static PROVIDER_REPUTATION_SCORE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "provider_reputation_score",
            "Current reputation score per provider",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("gauge provider_reputation_score: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry provider_reputation_score: {e}"));
    g
});

pub static RECEIPT_PERSIST_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("receipt_persist_fail_total", "Receipt persistence failures")
        .unwrap_or_else(|e| panic!("counter receipt persist fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry receipt persist fail: {e}"));
    c.handle()
});

pub static MATCH_LOOP_LATENCY_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new("match_loop_latency_seconds", "Settlement loop latency");
    let h = HistogramVec::new(opts, &["lane"])
        .unwrap_or_else(|e| panic!("histogram match loop latency: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry match loop latency: {e}"));
    h
});

pub static SETTLE_APPLIED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("settle_applied_total", "Receipts applied")
        .unwrap_or_else(|e| panic!("counter settle_applied_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry settle_applied_total: {e}"));
    c.handle()
});

pub static SETTLE_FAILED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("settle_failed_total", "Settlement failures"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter settle_failed_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry settle_failed_total: {e}"));
    c
});

pub static SETTLE_MODE_CHANGE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("settle_mode_change_total", "Settlement mode changes"),
        &["to"],
    )
    .unwrap_or_else(|e| panic!("counter settle_mode_change_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry settle_mode_change_total: {e}"));
    c
});

pub static SETTLE_AUDIT_MISMATCH_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "settle_audit_mismatch_total",
        "Receipts failing settlement audit",
    )
    .unwrap_or_else(|e| panic!("counter settle_audit_mismatch_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry settle_audit_mismatch_total: {e}"));
    c.handle()
});

pub static GOV_PROPOSALS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("gov_proposals_total", "Governance proposals by status"),
        &["status"],
    )
    .unwrap_or_else(|e| panic!("counter gov_proposals_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry gov_proposals_total: {e}"));
    c
});

pub static GOV_VOTES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("gov_votes_total", "Governance votes"),
        &["choice"],
    )
    .unwrap_or_else(|e| panic!("counter gov_votes_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry gov_votes_total: {e}"));
    c
});

pub static RELEASE_VOTES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("release_votes_total", "Release votes by choice"),
        &["choice"],
    )
    .unwrap_or_else(|e| panic!("counter release_votes_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry release_votes_total: {e}"));
    c
});

pub static RELEASE_INSTALLS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "release_installs_total",
        "Nodes booted with governance-approved releases",
    )
    .unwrap_or_else(|e| panic!("counter release_installs_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry release_installs_total: {e}"));
    c.handle()
});

pub static RELEASE_QUORUM_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "release_quorum_fail_total",
        "Release submissions rejected due to insufficient provenance signatures",
    )
    .unwrap_or_else(|e| panic!("counter release_quorum_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry release_quorum_fail_total: {e}"));
    c.handle()
});

pub static GOV_ACTIVATION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("gov_activation_total", "Governance activations"),
        &["key"],
    )
    .unwrap_or_else(|e| panic!("counter gov_activation_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry gov_activation_total: {e}"));
    c
});

pub static GOV_ROLLBACK_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("gov_rollback_total", "Governance rollbacks"),
        &["key"],
    )
    .unwrap_or_else(|e| panic!("counter gov_rollback_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry gov_rollback_total: {e}"));
    c
});

pub static GOV_ACTIVATION_DELAY_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let h = HistogramVec::new(
        HistogramOpts::new(
            "gov_activation_delay_seconds",
            "Delay between scheduled and actual activation",
        ),
        &["key"],
    )
    .unwrap_or_else(|e| panic!("histogram gov_activation_delay_seconds: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry gov_activation_delay_seconds: {e}"));
    h
});

pub static GOV_DEPENDENCY_POLICY_ALLOWED: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "gov_dependency_policy_allowed",
            "Governance-approved dependency entries (1 allowed / 0 disallowed)",
        ),
        &["kind", "label"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry gov_dependency_policy_allowed: {e}"));
    g
});

/// Send governance events to an external webhook if `GOV_WEBHOOK_URL` is set.
pub fn governance_webhook(event: &str, proposal_id: u64) {
    if let Ok(url) = std::env::var("GOV_WEBHOOK_URL") {
        let payload = GovernanceWebhookPayload { event, proposal_id };
        let _ = GOV_WEBHOOK_CLIENT
            .request(Method::Post, &url)
            .and_then(|req| req.json(&payload))
            .and_then(|req| req.send());
    }
}

pub fn update_ad_budget_metrics(snapshot: &ad_market::BudgetBrokerSnapshot) {
    #[cfg(feature = "telemetry")]
    {
        let config = &snapshot.config;
        let analytics = ad_market::budget_snapshot_analytics(snapshot);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["epoch_impressions"])
            .unwrap_or_else(|e| panic!("budget config epoch impressions: {e}"))
            .set(config.epoch_impressions as f64);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["step_size"])
            .unwrap_or_else(|e| panic!("budget config step size: {e}"))
            .set(config.step_size);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["dual_step"])
            .unwrap_or_else(|e| panic!("budget config dual step: {e}"))
            .set(config.dual_step);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["dual_forgetting"])
            .unwrap_or_else(|e| panic!("budget config dual forgetting: {e}"))
            .set(config.dual_forgetting);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["max_kappa"])
            .unwrap_or_else(|e| panic!("budget config max kappa: {e}"))
            .set(config.max_kappa);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["min_kappa"])
            .unwrap_or_else(|e| panic!("budget config min kappa: {e}"))
            .set(config.min_kappa);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["shadow_price_cap"])
            .unwrap_or_else(|e| panic!("budget config shadow price cap: {e}"))
            .set(config.shadow_price_cap);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["smoothing"])
            .unwrap_or_else(|e| panic!("budget config smoothing: {e}"))
            .set(config.smoothing);
        AD_BUDGET_CONFIG_VALUES
            .ensure_handle_for_label_values(&["epochs_per_budget"])
            .unwrap_or_else(|e| panic!("budget config epochs per budget: {e}"))
            .set(config.epochs_per_budget as f64);

        AD_BUDGET_SNAPSHOT_GENERATED_AT
            .set(snapshot.generated_at_micros.min(i64::MAX as u64) as i64);

        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["campaign_count"])
            .unwrap_or_else(|e| panic!("budget summary campaign count: {e}"))
            .set(analytics.campaign_count as f64);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["cohort_count"])
            .unwrap_or_else(|e| panic!("budget summary cohort count: {e}"))
            .set(analytics.cohort_count as f64);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["mean_kappa"])
            .unwrap_or_else(|e| panic!("budget summary mean kappa: {e}"))
            .set(analytics.mean_kappa);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["max_kappa"])
            .unwrap_or_else(|e| panic!("budget summary max kappa: {e}"))
            .set(analytics.max_kappa);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["mean_smoothed_error"])
            .unwrap_or_else(|e| panic!("budget summary mean error: {e}"))
            .set(analytics.mean_smoothed_error);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["max_abs_smoothed_error"])
            .unwrap_or_else(|e| panic!("budget summary max abs error: {e}"))
            .set(analytics.max_abs_smoothed_error);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["realized_spend_total_usd"])
            .unwrap_or_else(|e| panic!("budget summary realized spend: {e}"))
            .set(analytics.realized_spend_total);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["epoch_target_total_usd"])
            .unwrap_or_else(|e| panic!("budget summary epoch target: {e}"))
            .set(analytics.epoch_target_total);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["epoch_spend_total_usd"])
            .unwrap_or_else(|e| panic!("budget summary epoch spend: {e}"))
            .set(analytics.epoch_spend_total);
        AD_BUDGET_SUMMARY_VALUES
            .ensure_handle_for_label_values(&["dual_price_max"])
            .unwrap_or_else(|e| panic!("budget summary dual price max: {e}"))
            .set(analytics.dual_price_max);

        let mut new_campaigns = HashSet::with_capacity(snapshot.campaigns.len());
        let mut new_cohorts = HashSet::new();
        for campaign in &snapshot.campaigns {
            new_campaigns.insert(campaign.campaign_id.clone());
            let labels = [campaign.campaign_id.as_str()];
            AD_BUDGET_CAMPAIGN_REMAINING_USD
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("budget remaining labels: {e}"))
                .set(campaign.remaining_budget as f64);
            AD_BUDGET_CAMPAIGN_DUAL_PRICE
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("budget dual price labels: {e}"))
                .set(campaign.dual_price);
            AD_BUDGET_CAMPAIGN_EPOCH_TARGET_USD
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("budget epoch target labels: {e}"))
                .set(campaign.epoch_target);

            for cohort in &campaign.cohorts {
                let domain = cohort.cohort.domain.clone();
                let provider = cohort
                    .cohort
                    .provider
                    .clone()
                    .unwrap_or_else(|| "-".to_string());
                let badges = if cohort.cohort.badges.is_empty() {
                    "none".to_string()
                } else {
                    cohort.cohort.badges.join("|")
                };
                let labels = [
                    campaign.campaign_id.as_str(),
                    domain.as_str(),
                    provider.as_str(),
                    badges.as_str(),
                ];
                AD_BUDGET_COHORT_KAPPA
                    .ensure_handle_for_label_values(&labels)
                    .unwrap_or_else(|e| panic!("budget cohort kappa labels: {e}"))
                    .set(cohort.kappa);
                AD_BUDGET_COHORT_ERROR
                    .ensure_handle_for_label_values(&labels)
                    .unwrap_or_else(|e| panic!("budget cohort error labels: {e}"))
                    .set(cohort.smoothed_error);
                AD_BUDGET_COHORT_REALIZED_USD
                    .ensure_handle_for_label_values(&labels)
                    .unwrap_or_else(|e| panic!("budget cohort realized labels: {e}"))
                    .set(cohort.realized_spend);
                new_cohorts.insert((campaign.campaign_id.clone(), domain, provider, badges));
            }
        }

        let mut active_campaigns = AD_BUDGET_CAMPAIGN_LABELS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous_campaigns: Vec<String> = active_campaigns.iter().cloned().collect();
        for campaign in &previous_campaigns {
            if !new_campaigns.contains(campaign) {
                let labels = [campaign.as_str()];
                let _ = AD_BUDGET_CAMPAIGN_REMAINING_USD.remove_label_values(&labels);
                let _ = AD_BUDGET_CAMPAIGN_DUAL_PRICE.remove_label_values(&labels);
                let _ = AD_BUDGET_CAMPAIGN_EPOCH_TARGET_USD.remove_label_values(&labels);
            }
        }
        active_campaigns.clear();
        active_campaigns.extend(new_campaigns);

        let mut active_cohorts = AD_BUDGET_COHORT_LABELS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous_cohorts: Vec<(String, String, String, String)> =
            active_cohorts.iter().cloned().collect();
        for cohort in &previous_cohorts {
            if !new_cohorts.contains(cohort) {
                let labels = [
                    cohort.0.as_str(),
                    cohort.1.as_str(),
                    cohort.2.as_str(),
                    cohort.3.as_str(),
                ];
                let _ = AD_BUDGET_COHORT_KAPPA.remove_label_values(&labels);
                let _ = AD_BUDGET_COHORT_ERROR.remove_label_values(&labels);
                let _ = AD_BUDGET_COHORT_REALIZED_USD.remove_label_values(&labels);
            }
        }
        active_cohorts.clear();
        active_cohorts.extend(new_cohorts);
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = snapshot;
    }
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;

    #[test]
    fn ad_budget_summary_and_config_metrics_are_populated() {
        AD_BUDGET_SUMMARY_VALUES.reset();
        AD_BUDGET_CONFIG_VALUES.reset();
        AD_BUDGET_COHORT_KAPPA.reset();
        AD_BUDGET_COHORT_ERROR.reset();
        AD_BUDGET_COHORT_REALIZED_USD.reset();
        let mut config = ad_market::BudgetBrokerConfig::default();
        config.epoch_impressions = 4;
        config.step_size = 0.08;
        config.dual_step = 0.03;
        config.dual_forgetting = 0.5;
        config.max_kappa = 1.5;
        config.min_kappa = 0.4;
        config.shadow_price_cap = 3.0;
        config.smoothing = 0.2;
        let snapshot = ad_market::BudgetBrokerSnapshot {
            generated_at_micros: 42,
            config: config.clone(),
            campaigns: vec![ad_market::CampaignBudgetSnapshot {
                campaign_id: "cmp-test".into(),
                total_budget: 2_000_000,
                remaining_budget: 1_000_000,
                epoch_target: 500_000.0,
                epoch_spend: 450_000.0,
                epoch_impressions: 3,
                dual_price: 0.75,
                cohorts: vec![ad_market::CohortBudgetSnapshot {
                    cohort: ad_market::CohortKeySnapshot {
                        domain: "example.com".into(),
                        provider: Some("wallet".into()),
                        badges: vec!["badge-a".into()],
                    },
                    kappa: 0.85,
                    smoothed_error: 0.12,
                    realized_spend: 220_000.0,
                }],
            }],
        };

        update_ad_budget_metrics(&snapshot);

        let campaign_count = AD_BUDGET_SUMMARY_VALUES
            .get_metric_with_label_values(&["campaign_count"])
            .expect("campaign_count gauge");
        assert_eq!(campaign_count.get(), 1.0);

        let cohort_count = AD_BUDGET_SUMMARY_VALUES
            .get_metric_with_label_values(&["cohort_count"])
            .expect("cohort_count gauge");
        assert_eq!(cohort_count.get(), 1.0);

        let mean_kappa = AD_BUDGET_SUMMARY_VALUES
            .get_metric_with_label_values(&["mean_kappa"])
            .expect("mean kappa gauge");
        assert!((mean_kappa.get() - 0.85).abs() < f64::EPSILON);

        let realized_total = AD_BUDGET_SUMMARY_VALUES
            .get_metric_with_label_values(&["realized_spend_total_usd"])
            .expect("realized spend total gauge");
        assert!((realized_total.get() - 220_000.0).abs() < f64::EPSILON);

        let dual_step = AD_BUDGET_CONFIG_VALUES
            .get_metric_with_label_values(&["dual_step"])
            .expect("dual_step gauge");
        assert!((dual_step.get() - config.dual_step).abs() < f64::EPSILON);

        let min_kappa = AD_BUDGET_CONFIG_VALUES
            .get_metric_with_label_values(&["min_kappa"])
            .expect("min_kappa gauge");
        assert!((min_kappa.get() - config.min_kappa).abs() < f64::EPSILON);

        let cohort_labels = ["cmp-test", "example.com", "wallet", "badge-a"];
        let cohort_kappa = AD_BUDGET_COHORT_KAPPA
            .get_metric_with_label_values(&cohort_labels)
            .expect("cohort kappa gauge");
        assert!((cohort_kappa.get() - 0.85).abs() < f64::EPSILON);

        let cohort_realized = AD_BUDGET_COHORT_REALIZED_USD
            .get_metric_with_label_values(&cohort_labels)
            .expect("cohort realized spend gauge");
        assert!((cohort_realized.get() - 220_000.0).abs() < f64::EPSILON);

        let cohort_error = AD_BUDGET_COHORT_ERROR
            .get_metric_with_label_values(&cohort_labels)
            .expect("cohort error gauge");
        assert!((cohort_error.get() - 0.12).abs() < f64::EPSILON);

        let remaining_budget = AD_BUDGET_CAMPAIGN_REMAINING_USD
            .get_metric_with_label_values(&["cmp-test"])
            .expect("campaign remaining gauge");
        assert!((remaining_budget.get() - 1_000_000.0).abs() < f64::EPSILON);

        let dual_price = AD_BUDGET_CAMPAIGN_DUAL_PRICE
            .get_metric_with_label_values(&["cmp-test"])
            .expect("campaign dual price gauge");
        assert!((dual_price.get() - 0.75).abs() < f64::EPSILON);
    }
}

pub fn update_ad_market_utilization_metrics(
    cohorts: &[crate::ad_readiness::AdReadinessCohortUtilization],
) {
    #[cfg(feature = "telemetry")]
    {
        let mut new_labels: HashSet<(String, String, String)> =
            HashSet::with_capacity(cohorts.len());
        for entry in cohorts {
            let domain_label = entry.domain.clone();
            let provider_label = entry.provider.clone().unwrap_or_else(|| "none".to_string());
            let badges_label = if entry.badges.is_empty() {
                "none".to_string()
            } else {
                entry.badges.join("|")
            };
            let labels = [
                domain_label.as_str(),
                provider_label.as_str(),
                badges_label.as_str(),
            ];
            AD_MARKET_UTILIZATION_OBSERVED
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("ad market utilization observed labels: {e}"))
                .set(i64::from(entry.observed_utilization_ppm));
            AD_MARKET_UTILIZATION_TARGET
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("ad market utilization target labels: {e}"))
                .set(i64::from(entry.target_utilization_ppm));
            AD_MARKET_UTILIZATION_DELTA
                .ensure_handle_for_label_values(&labels)
                .unwrap_or_else(|e| panic!("ad market utilization delta labels: {e}"))
                .set(entry.delta_ppm);
            new_labels.insert((domain_label, provider_label, badges_label));
        }
        let mut active = AD_MARKET_UTILIZATION_LABELS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous: Vec<(String, String, String)> = active.iter().cloned().collect();
        for label in previous {
            if !new_labels.contains(&label) {
                let values = [label.0.as_str(), label.1.as_str(), label.2.as_str()];
                let _ = AD_MARKET_UTILIZATION_OBSERVED.remove_label_values(&values);
                let _ = AD_MARKET_UTILIZATION_TARGET.remove_label_values(&values);
                let _ = AD_MARKET_UTILIZATION_DELTA.remove_label_values(&values);
            }
        }
        active.clear();
        active.extend(new_labels);
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = cohorts;
    }
}

pub static GOV_OPEN_PROPOSALS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("gov_open_proposals", "Open governance proposals")
        .unwrap_or_else(|e| panic!("gauge gov_open_proposals: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry gov_open_proposals: {e}"));
    g.handle()
});

pub static GOV_PROPOSALS_PENDING: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "gov_proposals_pending",
        "Governance proposals pending activation",
    )
    .unwrap_or_else(|e| panic!("gauge gov_proposals_pending: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry gov_proposals_pending: {e}"));
    g.handle()
});

pub static GOV_QUORUM_REQUIRED: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("gov_quorum_required", "Governance quorum")
        .unwrap_or_else(|e| panic!("gauge gov_quorum_required: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry gov_quorum_required: {e}"));
    g.handle()
});

pub static RECEIPT_CORRUPT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("receipt_corrupt_total", "Corrupted receipt entries on load")
        .unwrap_or_else(|e| panic!("counter receipt corrupt: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry receipt corrupt: {e}"));
    c.handle()
});

pub static WAL_CORRUPT_RECOVERY_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "wal_corrupt_recovery_total",
        "WAL entries skipped due to checksum mismatch",
    )
    .unwrap_or_else(|e| panic!("counter wal corrupt recovery: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry wal corrupt recovery: {e}"));
    c.handle()
});

pub static IDENTITY_REGISTRATIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "identity_registrations_total",
            "Handle registration attempts",
        ),
        &["status"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static IDENTITY_HANDLE_NORMALIZATION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "identity_handle_normalization_total",
            "Handle normalization outcomes grouped by accuracy",
        ),
        &["accuracy"],
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static IDENTITY_REPLAYS_BLOCKED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "identity_replays_blocked_total",
        "Rejected identity replay attempts",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static IDENTITY_NONCE_SKIPS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "identity_nonce_skips_total",
        "Non-contiguous nonce submissions",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static DUP_TX_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("dup_tx_reject_total", "Transactions rejected as duplicate")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static TX_ADMITTED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("tx_admitted_total", "Total admitted transactions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static TX_SUBMITTED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("tx_submitted_total", "Total submitted transactions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static TX_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("tx_rejected_total", "Total rejected transactions"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TX_JURISDICTION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("tx_jurisdiction_total", "Transactions by jurisdiction"),
        &["jurisdiction"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static BLOCK_MINED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("block_mined_total", "Total mined blocks")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static BLOCK_APPLY_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "block_apply_fail_total",
        "Blocks that failed atomic application",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static GOSSIP_CONVERGENCE_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "gossip_convergence_seconds",
        "Time for all peers to agree on the network tip",
    );
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h.handle()
});

pub static FORK_REORG_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("fork_reorg_total", "Total observed fork reorgs"),
        &["depth"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TTL_DROP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "ttl_drop_total",
        "Transactions dropped due to TTL expiration",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static GOSSIP_TTL_DROP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "gossip_ttl_drop_total",
        "Gossip dedup entries removed due to TTL expiry",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static STARTUP_TTL_DROP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "startup_ttl_drop_total",
        "Expired mempool entries dropped during startup",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static SESSION_KEY_ISSUED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("session_key_issued_total", "Session keys issued")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static SESSION_KEY_EXPIRED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "session_key_expired_total",
        "Expired session keys encountered",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static LOCK_POISON_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "lock_poison_total",
        "Lock acquisition failures due to poisoning",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static BANNED_PEERS_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("banned_peers_total", "Total peers currently banned")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static BANNED_PEER_EXPIRATION: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "banned_peer_expiration_seconds",
            "Expiration timestamp for active peer bans",
        ),
        &["peer"],
    )
    .unwrap_or_else(|e| panic!("gauge vec: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static COURIER_FLUSH_ATTEMPT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "courier_flush_attempt_total",
        "Total courier receipt flush attempts",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static COURIER_FLUSH_FAILURE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "courier_flush_failure_total",
        "Failed courier receipt flush attempts",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static ORPHAN_SWEEP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "orphan_sweep_total",
        "Transactions dropped because the sender account is missing",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static INVALID_SELECTOR_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "invalid_selector_reject_total",
        "Transactions rejected for invalid fee selector",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static BALANCE_OVERFLOW_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "balance_overflow_reject_total",
        "Transactions rejected due to balance overflow",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static DROP_NOT_FOUND_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "drop_not_found_total",
        "drop_transaction failures for missing entries",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static PEER_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("peer_error_total", "Total peer errors grouped by code"),
        &["code"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_REQUEST_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("peer_request_total", "Total requests received from peer"),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_BYTES_SENT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("peer_bytes_sent_total", "Bytes sent to peer"),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_DROP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("peer_drop_total", "Messages dropped grouped by reason"),
        &["peer_id", "reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static MESH_PEER_CONNECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("mesh_peer_connected_total", "Total mesh peers discovered"),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static MESH_PEER_LATENCY_MS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("mesh_peer_latency_ms", "Mesh peer latency in milliseconds"),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static P2P_REQUEST_LIMIT_HITS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "p2p_request_limit_hits_total",
            "Per-peer hits on the request rate limiter",
        ),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_RATE_LIMIT_TOTAL: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new("peer_rate_limit_total", "Rate limit drops per peer"),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec peer_rate_limit_total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry peer_rate_limit_total: {e}"));
    g
});

pub static PEER_METRICS_ACTIVE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "peer_metrics_active",
        "Number of peers currently tracked for telemetry",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static OVERLAY_BACKEND_ACTIVE: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "overlay_backend_active",
            "Indicator gauge for the active overlay backend (1 active / 0 inactive)",
        ),
        &["backend"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec overlay_backend_active: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry overlay_backend_active: {e}"));
    g
});

pub static OVERLAY_PEER_TOTAL: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "overlay_peer_total",
            "Overlay peers currently tracked by the uptime service, grouped by backend",
        ),
        &["backend"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec overlay_peer_total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry overlay_peer_total: {e}"));
    g
});

pub static OVERLAY_PEER_PERSISTED_TOTAL: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "overlay_peer_persisted_total",
            "Persisted overlay peer records grouped by backend",
        ),
        &["backend"],
    )
    .unwrap_or_else(|e| panic!("gauge_vec overlay_peer_persisted_total: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry overlay_peer_persisted_total: {e}"));
    g
});

pub static PEER_METRICS_SUBSCRIBERS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "peer_metrics_subscribers",
        "Active peer metrics websocket subscribers",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static PEER_METRICS_MEM_BYTES: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "peer_metrics_memory_bytes",
        "Approximate memory used by peer metrics map",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static REPUTATION_GOSSIP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "reputation_gossip_total",
            "Reputation gossip updates processed grouped by result",
        ),
        &["result"],
    )
    .unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static REPUTATION_GOSSIP_LATENCY_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let h = Histogram::with_opts(HistogramOpts::new(
        "reputation_gossip_latency_seconds",
        "Propagation latency for reputation updates",
    ))
    .unwrap_or_else(|e| panic!("histogram reputation_gossip_latency_seconds: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry reputation_gossip_latency_seconds: {e}"));
    h.handle()
});

pub static REPUTATION_GOSSIP_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "reputation_gossip_fail_total",
        "Reputation updates that failed verification or were stale",
    )
    .unwrap_or_else(|e| panic!("counter reputation_gossip_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry reputation_gossip_fail_total: {e}"));
    c.handle()
});

pub static AMM_SWAP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("amm_swap_total", "Total AMM swaps executed").unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry amm_swap_total: {e}"));
    c.handle()
});

pub static LIQUIDITY_REWARDS_DISBURSED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "liquidity_rewards_disbursed_total",
        "Liquidity mining rewards distributed",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry liquidity_rewards_disbursed_total: {e}"));
    c.handle()
});

pub static REBATE_CLAIMS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("rebate_claims_total", "Peer rebate claims submitted").unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry rebate_claims_total: {e}"));
    c.handle()
});

pub static REBATE_ISSUED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("rebate_issued_total", "Rebate vouchers issued").unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry rebate_issued_total: {e}"));
    c.handle()
});

pub static BUILD_PROVENANCE_VALID_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "build_provenance_valid_total",
        "Build provenance checks that succeeded",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry build_provenance_valid_total: {e}"));
    c.handle()
});

pub static BUILD_PROVENANCE_INVALID_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "build_provenance_invalid_total",
        "Build provenance checks that failed",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry build_provenance_invalid_total: {e}"));
    c.handle()
});

pub static PEER_METRICS_DROPPED: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "peer_metrics_dropped_total",
        "Websocket peer metrics frames dropped",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static AGGREGATOR_INGEST_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("aggregator_ingest_total", "Total peer metric ingests").unwrap();
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static CLUSTER_PEER_ACTIVE_TOTAL: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "cluster_peer_active_total",
        "Unique peers tracked by aggregator",
    )
    .unwrap();
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static PEER_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("peer_rejected_total", "Peers rejected grouped by reason"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_HANDSHAKE_FAIL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "net_peer_handshake_fail_total",
            "QUIC handshake failures per peer grouped by reason",
        ),
        &["peer_id", "reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_HANDSHAKE_SUCCESS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "net_peer_handshake_success_total",
            "Successful handshakes per peer",
        ),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_TLS_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "net_peer_tls_error_total",
            "TLS errors encountered per peer",
        ),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static HANDSHAKE_FAIL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "handshake_fail_total",
            "Handshake failures grouped by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_STATS_RESET_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_stats_reset_total",
            "Peer metric resets grouped by peer",
        ),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_STATS_QUERY_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_stats_query_total",
            "Peer metric queries grouped by peer",
        ),
        &["peer_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_REPUTATION_SCORE: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new("peer_reputation_score", "Peer reputation score"),
        &["peer_id"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static PEER_STATS_EXPORT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_stats_export_total",
            "Peer metric export attempts grouped by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_STATS_EXPORT_ALL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_stats_export_all_total",
            "Bulk peer metric export attempts grouped by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_STATS_EXPORT_VALIDATE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_stats_export_validate_total",
            "Validation results for exported peer metric archives",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_THROTTLE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_throttle_total",
            "Peer throttle events grouped by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_BACKPRESSURE_ACTIVE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_backpressure_active_total",
            "Backpressure activations grouped by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_BACKPRESSURE_DROPPED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_backpressure_dropped_total",
            "Requests dropped due to backpressure grouped by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static PEER_KEY_ROTATE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "peer_key_rotate_total",
            "Peer key rotation attempts grouped by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static KEY_ROTATION_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("key_rotation_total", "Successful peer key rotations")
        .unwrap_or_else(|e| panic!("counter key rotation: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry key rotation: {e}"));
    c.handle()
});

pub static CONFIG_RELOAD_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "config_reload_total",
            "Configuration reload attempts grouped by result",
        ),
        &["result"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static CONFIG_RELOAD_LAST_TS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "config_reload_last_ts",
        "Unix timestamp of the last successful config reload",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static GATEWAY_DNS_LOOKUP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "gateway_dns_lookup_total",
            "Gateway DNS verification attempts grouped by status",
        ),
        &["status"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static DNS_VERIFICATION_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "dns_verification_fail_total",
        "Total DNS verification failures",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static GOSSIP_DUPLICATE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "gossip_duplicate_total",
        "Duplicate gossip messages dropped",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static GOSSIP_FANOUT_GAUGE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("gossip_fanout_gauge", "Current gossip fanout")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static GOSSIP_LATENCY_BUCKETS: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "gossip_latency_seconds",
        "Observed latency hints used for adaptive gossip fanout",
    )
    .buckets(vec![
        0.000_5, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0,
    ]);
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h.handle()
});

pub static GOSSIP_PEER_FAILURE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "gossip_peer_failure_total",
            "Reasons peers were skipped during gossip fanout",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static RPC_CLIENT_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "rpc_client_error_total",
            "Total RPC client errors grouped by code",
        ),
        &["code"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static RUNTIME_SPAWN_LATENCY_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let buckets = telemetry::exponential_buckets(0.0005, 2.0, 18);
    let opts = HistogramOpts::new(
        "runtime_spawn_latency_seconds",
        "Latency observed when spawning tasks on the runtime",
    )
    .buckets(buckets);
    let hist = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(hist.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    hist.handle()
});

pub static RUNTIME_PENDING_TASKS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let gauge = IntGauge::new(
        "runtime_pending_tasks",
        "Pending async tasks managed by the runtime",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(gauge.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    gauge.handle()
});

pub static REMOTE_SIGNER_REQUEST_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "remote_signer_request_total",
        "Total remote signer requests",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static REMOTE_SIGNER_LATENCY_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let h = Histogram::with_opts(HistogramOpts::new(
        "remote_signer_latency_seconds",
        "Remote signer latency",
    ))
    .unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h.handle()
});

pub static REMOTE_SIGNER_SUCCESS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "remote_signer_success_total",
        "Successful remote signer responses",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static REMOTE_SIGNER_KEY_ROTATION_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "remote_signer_key_rotation_total",
        "Remote signer key rotations",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});
pub static PRIVACY_SANITIZATION_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("privacy_sanitization_total", "Total sanitized payloads")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static REMOTE_SIGNER_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "remote_signer_error_total",
            "Total remote signer errors grouped by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static RPC_TOKENS: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "rpc_tokens_available",
            "Current RPC rate limiter tokens per client",
        ),
        &["client"],
    );
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static RPC_BANS_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("rpc_bans_total", "Total RPC bans issued")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static RPC_RATE_LIMIT_ATTEMPT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "rpc_rate_limit_attempt_total",
        "RPC requests checked against the rate limiter",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static RPC_RATE_LIMIT_REJECT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "rpc_rate_limit_reject_total",
        "RPC requests rejected by the rate limiter",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c.handle()
});

pub static P2P_HANDSHAKE_REJECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "p2p_handshake_reject_total",
            "Handshakes rejected by reason",
        ),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static P2P_HANDSHAKE_ACCEPT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "p2p_handshake_accept_total",
            "Successful handshakes by feature mask",
        ),
        &["features"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static QUIC_CONN_LATENCY_SECONDS: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "quic_conn_latency_seconds",
        "QUIC connection handshake latency",
    );
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram quic latency: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry quic latency: {e}"));
    h.handle()
});

pub static QUIC_BYTES_SENT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("quic_bytes_sent_total", "Total bytes sent over QUIC")
        .unwrap_or_else(|e| panic!("counter quic bytes sent: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic bytes sent: {e}"));
    c.handle()
});

pub static QUIC_BYTES_RECV_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("quic_bytes_recv_total", "Total bytes received over QUIC")
        .unwrap_or_else(|e| panic!("counter quic bytes recv: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic bytes recv: {e}"));
    c.handle()
});

pub static QUIC_HANDSHAKE_FAIL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "quic_handshake_fail_total",
            "Total QUIC handshake failures by peer and reason",
        ),
        &["peer", "reason"],
    )
    .unwrap_or_else(|e| panic!("counter vec quic handshake fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic handshake fail: {e}"));
    c
});

pub static QUIC_PROVIDER_CONNECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "quic_provider_connect_total",
            "Successful QUIC connection events by provider",
        ),
        &["provider"],
    )
    .unwrap_or_else(|e| panic!("counter vec quic provider connect: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic provider connect: {e}"));
    c
});

pub static QUIC_CERT_ROTATION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "quic_cert_rotation_total",
            "Total QUIC certificate rotations by peer",
        ),
        &["peer"],
    )
    .unwrap_or_else(|e| panic!("counter vec quic cert rotate: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic cert rotate: {e}"));
    c
});

pub static QUIC_RETRANSMIT_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new("quic_retransmit_total", "Total QUIC packet retransmissions")
        .unwrap_or_else(|e| panic!("counter quic retransmit: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic retransmit: {e}"));
    c.handle()
});

pub static QUIC_DISCONNECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("quic_disconnect_total", "QUIC disconnects by error code"),
        &["code"],
    )
    .unwrap_or_else(|e| panic!("counter vec quic disconnect: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic disconnect: {e}"));
    c
});

pub static QUIC_ENDPOINT_REUSE_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "quic_endpoint_reuse_total",
        "Total QUIC endpoint reuse count",
    )
    .unwrap_or_else(|e| panic!("counter quic endpoint reuse: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic endpoint reuse: {e}"));
    c.handle()
});

pub static QUIC_FALLBACK_TCP_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "quic_fallback_tcp_total",
        "Total times QUIC connections fell back to TCP",
    )
    .unwrap_or_else(|e| panic!("counter quic fallback tcp: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry quic fallback tcp: {e}"));
    c.handle()
});

pub static BADGE_ACTIVE: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new("badge_active", "Whether a service badge is active (1/0)")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub static BADGE_LAST_CHANGE_SECONDS: Lazy<IntGaugeHandle> = Lazy::new(|| {
    let g = IntGauge::new(
        "badge_last_change_seconds",
        "Unix timestamp of the last badge mint/burn",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g.handle()
});

pub struct Recorder;

impl Recorder {
    pub fn tx_submitted(&self) {
        TX_SUBMITTED_TOTAL.inc();
    }

    pub fn tx_rejected(&self, reason: &str) {
        TX_REJECTED_TOTAL
            .ensure_handle_for_label_values(&[reason])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
    }

    pub fn block_mined(&self) {
        BLOCK_MINED_TOTAL.inc();
    }

    pub fn tx_jurisdiction(&self, j: &str) {
        TX_JURISDICTION_TOTAL
            .ensure_handle_for_label_values(&[j])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
    }
}

pub static RECORDER: Recorder = Recorder;

pub const LOG_FIELDS: &[&str] = &[
    "subsystem",
    "op",
    "sender",
    "nonce",
    "reason",
    "code",
    "fpb",
];

pub static LOG_EMIT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("log_emit_total", "Total emitted log events"),
        &["subsystem"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

#[cfg(feature = "telemetry")]
static TLS_WARNING_SINK: Lazy<Mutex<Option<http_env::TlsEnvWarningSinkGuard>>> =
    Lazy::new(|| Mutex::new(None));
#[cfg(feature = "telemetry")]
static TLS_WARNING_SUBSCRIBER: Lazy<Mutex<Option<DiagnosticsSubscriberGuard>>> =
    Lazy::new(|| Mutex::new(None));

pub static TLS_ENV_WARNING_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "tls_env_warning_total",
            "TLS environment configuration warnings grouped by prefix and code",
        ),
        &["prefix", "code"],
    )
    .unwrap_or_else(|e| panic!("counter tls env warning: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry tls env warning: {e}"));
    c
});

#[cfg(feature = "telemetry")]
pub static TLS_ENV_WARNING_EVENTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "tls_env_warning_events_total",
            "TLS environment configuration warnings grouped by prefix, code, and origin",
        ),
        &["prefix", "code", "origin"],
    )
    .unwrap_or_else(|e| panic!("counter tls env warning events: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry tls env warning events: {e}"));
    c
});

#[cfg(feature = "telemetry")]
pub static TLS_ENV_WARNING_LAST_SEEN_SECONDS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_last_seen_seconds",
            "Unix timestamp of the most recent TLS environment warning",
        ),
        &["prefix", "code"],
    )
    .unwrap_or_else(|e| panic!("gauge tls env warning last seen: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry tls env warning last seen: {e}"));
    g
});

#[cfg(not(feature = "telemetry"))]
pub static TLS_ENV_WARNING_LAST_SEEN_SECONDS: () = ();

#[cfg(not(feature = "telemetry"))]
pub static TLS_ENV_WARNING_EVENTS_TOTAL: () = ();

#[cfg(feature = "telemetry")]
pub static TLS_ENV_WARNING_DETAIL_FINGERPRINT: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_detail_fingerprint",
            "Fingerprint of the most recent TLS warning detail payload",
        ),
        &["prefix", "code"],
    )
    .unwrap_or_else(|e| panic!("gauge tls env warning detail fingerprint: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry tls env warning detail fingerprint: {e}"));
    g
});

#[cfg(not(feature = "telemetry"))]
pub static TLS_ENV_WARNING_DETAIL_FINGERPRINT: () = ();

#[cfg(feature = "telemetry")]
pub static TLS_ENV_WARNING_VARIABLES_FINGERPRINT: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "tls_env_warning_variables_fingerprint",
            "Fingerprint of the most recent TLS warning variable payload",
        ),
        &["prefix", "code"],
    )
    .unwrap_or_else(|e| panic!("gauge tls env warning variables fingerprint: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry tls env warning variables fingerprint: {e}"));
    g
});

#[cfg(not(feature = "telemetry"))]
pub static TLS_ENV_WARNING_VARIABLES_FINGERPRINT: () = ();

#[cfg(feature = "telemetry")]
pub fn tls_env_warning_detail_fingerprint(detail: &str) -> i64 {
    tls_detail_fingerprint(detail)
}

#[cfg(feature = "telemetry")]
pub fn tls_env_warning_variables_fingerprint(variables: &[String]) -> Option<i64> {
    tls_variables_fingerprint(variables.iter().map(|value| value.as_str()))
}

#[cfg(feature = "telemetry")]
pub fn record_tls_env_warning(
    prefix: &str,
    code: &str,
    origin: WarningOrigin,
    detail: Option<&str>,
    variables: &[String],
) {
    TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&[prefix, code])
        .expect(LABEL_REGISTRATION_ERR)
        .inc();
    if let Ok(handle) = TLS_ENV_WARNING_EVENTS_TOTAL.ensure_handle_for_label_values(&[
        prefix,
        code,
        origin.as_str(),
    ]) {
        handle.inc();
    }
    if let Ok(handle) =
        TLS_ENV_WARNING_LAST_SEEN_SECONDS.ensure_handle_for_label_values(&[prefix, code])
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        handle.set(now as i64);

        let detail_string = detail
            .filter(|d| !d.is_empty())
            .map(|value| value.to_string());
        let new_detail_fingerprint = detail_string
            .as_ref()
            .map(|value| tls_detail_fingerprint(value.as_str()));
        let variables_vec: Vec<String> = if variables.is_empty() {
            Vec::new()
        } else {
            variables.to_vec()
        };
        let new_variables_fingerprint =
            tls_variables_fingerprint(variables_vec.iter().map(|value| value.as_str()));
        let detail_bucket = fingerprint_label(new_detail_fingerprint);
        let variables_bucket = fingerprint_label(new_variables_fingerprint);

        let event = {
            let mut entry = TLS_ENV_WARNINGS
                .entry((prefix.to_string(), code.to_string()))
                .or_insert_with(LocalTlsWarning::default);

            entry.total = entry.total.saturating_add(1);
            entry.last_delta = 1;
            entry.last_seen = now;
            entry.origin = origin;

            let detail_changed = match detail_string {
                Some(detail_value) => {
                    let changed = entry.detail_fingerprint != new_detail_fingerprint;
                    entry.detail = Some(detail_value);
                    entry.detail_fingerprint = new_detail_fingerprint;
                    changed
                }
                None => false,
            };

            let variables_changed = if !variables_vec.is_empty() {
                let changed = entry.variables_fingerprint != new_variables_fingerprint;
                entry.variables = variables_vec.clone();
                entry.variables_fingerprint = new_variables_fingerprint;
                changed
            } else {
                false
            };

            *entry
                .detail_fingerprint_counts
                .entry(detail_bucket.clone())
                .or_insert(0) += 1;
            *entry
                .variables_fingerprint_counts
                .entry(variables_bucket.clone())
                .or_insert(0) += 1;

            TlsEnvWarningTelemetryEvent {
                prefix: prefix.to_string(),
                code: code.to_string(),
                origin,
                total: entry.total,
                last_delta: entry.last_delta,
                last_seen: entry.last_seen,
                detail: entry.detail.clone(),
                detail_fingerprint: entry.detail_fingerprint,
                detail_bucket: detail_bucket.clone(),
                detail_changed,
                variables: entry.variables.clone(),
                variables_fingerprint: entry.variables_fingerprint,
                variables_bucket: variables_bucket.clone(),
                variables_changed,
            }
        };

        if let Ok(handle) =
            TLS_ENV_WARNING_DETAIL_FINGERPRINT.ensure_handle_for_label_values(&[prefix, code])
        {
            handle.set(event.detail_fingerprint.unwrap_or(0));
        }

        if let Ok(handle) =
            TLS_ENV_WARNING_VARIABLES_FINGERPRINT.ensure_handle_for_label_values(&[prefix, code])
        {
            handle.set(event.variables_fingerprint.unwrap_or(0));
        }

        dispatch_tls_env_warning_event(&event);
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn record_tls_env_warning(
    _prefix: &str,
    _code: &str,
    _origin: WarningOrigin,
    _detail: Option<&str>,
    _variables: &[String],
) {
}

#[cfg(feature = "telemetry")]
pub fn ensure_tls_env_warning_diagnostics_bridge() {
    let mut guard = TLS_WARNING_SUBSCRIBER
        .lock()
        .expect("tls warning subscriber");
    if guard.is_none() {
        *guard = Some(install_tls_env_warning_subscriber(|warning| {
            if http_env::has_tls_warning_sinks() {
                return;
            }
            record_tls_env_warning(
                &warning.prefix,
                &warning.code,
                WarningOrigin::Diagnostics,
                Some(&warning.detail),
                &warning.variables,
            );
        }));
    }
}

#[cfg(feature = "telemetry")]
pub fn install_tls_env_warning_forwarder() {
    ensure_tls_env_warning_diagnostics_bridge();
    let mut guard = TLS_WARNING_SINK.lock().expect("tls warning sink");
    if guard.is_none() {
        *guard = Some(http_env::register_tls_warning_sink(|warning| {
            record_tls_env_warning(
                &warning.prefix,
                warning.code,
                WarningOrigin::Diagnostics,
                Some(&warning.detail),
                &warning.variables,
            );
        }));
    }
}

#[cfg(feature = "telemetry")]
pub fn reset_tls_env_warning_forwarder_for_testing() {
    if let Ok(mut sink) = TLS_WARNING_SINK.lock() {
        *sink = None;
    }
    if let Ok(mut subscriber) = TLS_WARNING_SUBSCRIBER.lock() {
        *subscriber = None;
    }
    reset_tls_env_warning_telemetry_sinks_for_test();
}

#[cfg(feature = "telemetry")]
pub fn tls_env_warning_snapshots() -> Vec<TlsEnvWarningSnapshot> {
    let mut snapshots = Vec::new();
    Lazy::force(&TLS_ENV_WARNINGS).for_each(|(prefix, code), value| {
        snapshots.push(TlsEnvWarningSnapshot {
            prefix: prefix.clone(),
            code: code.clone(),
            total: value.total,
            last_delta: value.last_delta,
            last_seen: value.last_seen,
            origin: value.origin,
            detail: value.detail.clone(),
            detail_fingerprint: value.detail_fingerprint,
            variables: value.variables.clone(),
            variables_fingerprint: value.variables_fingerprint,
            detail_fingerprint_counts: value.detail_fingerprint_counts.clone(),
            variables_fingerprint_counts: value.variables_fingerprint_counts.clone(),
        });
    });
    snapshots
}

#[cfg(not(feature = "telemetry"))]
pub fn install_tls_env_warning_forwarder() {}

#[cfg(not(feature = "telemetry"))]
pub fn ensure_tls_env_warning_diagnostics_bridge() {}

#[cfg(not(feature = "telemetry"))]
pub fn reset_tls_env_warning_forwarder_for_testing() {}

#[cfg(not(feature = "telemetry"))]
pub fn tls_env_warning_snapshots() -> Vec<TlsEnvWarningSnapshot> {
    Vec::new()
}

#[cfg(feature = "telemetry")]
pub fn clear_tls_env_warning_snapshots_for_testing() {
    TLS_ENV_WARNINGS.clear();
}

pub static LOG_DROP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new("log_drop_total", "Logs dropped due to rate limiting"),
        &["subsystem"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static LOG_ENTRIES_INDEXED_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "log_entries_indexed_total",
        "Total JSON log entries processed by the offline indexer",
    )
    .unwrap_or_else(|e| panic!("counter log_entries_indexed_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry log_entries_indexed_total: {e}"));
    c.handle()
});

pub static LOG_CORRELATION_INDEX_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        Opts::new(
            "log_correlation_index_total",
            "Indexed log entries grouped by correlation id",
        ),
        &["correlation_id"],
    )
    .unwrap_or_else(|e| panic!("counter_vec log_correlation_index_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry log_correlation_index_total: {e}"));
    c
});

pub static LOG_CORRELATION_FAIL_TOTAL: Lazy<IntCounterHandle> = Lazy::new(|| {
    let c = IntCounter::new(
        "log_correlation_fail_total",
        "Correlation lookups that returned no matching log entries",
    )
    .unwrap_or_else(|e| panic!("counter log_correlation_fail_total: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry log_correlation_fail_total: {e}"));
    c.handle()
});

pub static LOG_SIZE_BYTES: Lazy<HistogramHandle> = Lazy::new(|| {
    let opts = HistogramOpts::new("log_size_bytes", "Size of serialized log events in bytes")
        .buckets(telemetry::exponential_buckets(64.0, 2.0, 8));
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h.handle()
});

static LOG_SEC: AtomicU64 = AtomicU64::new(0);
static LOG_COUNT: AtomicU64 = AtomicU64::new(0);
static LOG_TOGGLES: Lazy<RwLock<HashMap<String, bool>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// Maximum log events per second before sampling kicks in.
pub const LOG_LIMIT: u64 = 100;
/// After `LOG_LIMIT` is exceeded, emit one in every `LOG_SAMPLE_STRIDE` events.
pub const LOG_SAMPLE_STRIDE: u64 = 100;

pub fn should_log(subsystem: &str) -> bool {
    if let Some(enabled) = LOG_TOGGLES.read().unwrap().get(subsystem) {
        if !*enabled {
            LOG_DROP_TOTAL
                .ensure_handle_for_label_values(&[subsystem])
                .expect(LABEL_REGISTRATION_ERR)
                .inc();
            return false;
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = LOG_SEC.load(Ordering::Relaxed);
    if now != last {
        LOG_SEC.store(now, Ordering::Relaxed);
        LOG_COUNT.store(0, Ordering::Relaxed);
    }
    let count = LOG_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= LOG_LIMIT || count % LOG_SAMPLE_STRIDE == 0 {
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&[subsystem])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
        true
    } else {
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&[subsystem])
            .expect(LABEL_REGISTRATION_ERR)
            .inc();
        false
    }
}

pub fn set_log_enabled(subsystem: &str, enabled: bool) {
    LOG_TOGGLES
        .write()
        .unwrap()
        .insert(subsystem.to_owned(), enabled);
}

pub fn observe_log_size(bytes: usize) {
    LOG_SIZE_BYTES.observe(bytes as f64);
}

#[doc(hidden)]
pub fn reset_log_counters() {
    LOG_SEC.store(0, Ordering::Relaxed);
    LOG_COUNT.store(0, Ordering::Relaxed);
    for sub in ["mempool", "storage", "p2p", "compute"] {
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&[sub])
            .expect(LABEL_REGISTRATION_ERR)
            .reset();
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&[sub])
            .expect(LABEL_REGISTRATION_ERR)
            .reset();
    }
}

pub fn redact_at_rest(dir: &str, hours: u64, hash: bool) -> PyResult<()> {
    use std::fs;
    use std::time::Duration;

    let cutoff = SystemTime::now() - Duration::from_secs(hours * 3600);
    for entry in fs::read_dir(dir).map_err(|e| PyError::runtime(e.to_string()))? {
        let entry = entry.map_err(|e| PyError::runtime(e.to_string()))?;
        let path = entry.path();
        if path.is_file() {
            let meta = entry
                .metadata()
                .map_err(|e| PyError::runtime(e.to_string()))?;
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    if hash {
                        let data = fs::read(&path).map_err(|e| PyError::runtime(e.to_string()))?;
                        let digest = blake3::hash(&data).to_hex().to_string();
                        fs::write(&path, digest).map_err(|e| PyError::runtime(e.to_string()))?;
                    } else {
                        let _ = fs::remove_file(&path).map_err(|e| PyError::runtime(e.to_string()));
                    }
                }
            }
        }
    }
    Ok(())
}

fn gather() -> String {
    init_wrapper_metrics();
    // Ensure all metrics are registered even if they haven't been used yet so
    // `gather_metrics` always exposes a stable set of counters.
    let _ = (
        MEMPOOL_SIZE
            .ensure_handle_for_label_values(&["consumer"])
            .expect(LABEL_REGISTRATION_ERR),
        MEMPOOL_SIZE
            .ensure_handle_for_label_values(&["industrial"])
            .expect(LABEL_REGISTRATION_ERR),
        &*EVICTIONS_TOTAL,
        &*FEE_FLOOR_REJECT_TOTAL,
        &*DUP_TX_REJECT_TOTAL,
        &*TX_ADMITTED_TOTAL,
        &*TX_SUBMITTED_TOTAL,
        &*TX_REJECTED_TOTAL,
        &*BLOCK_MINED_TOTAL,
        &*BLOCK_APPLY_FAIL_TOTAL,
        &*GOSSIP_CONVERGENCE_SECONDS,
        FORK_REORG_TOTAL
            .ensure_handle_for_label_values(&["0"])
            .expect(LABEL_REGISTRATION_ERR),
        &*TTL_DROP_TOTAL,
        &*STARTUP_TTL_DROP_TOTAL,
        &*GOSSIP_TTL_DROP_TOTAL,
        &*LOCK_POISON_TOTAL,
        &*ORPHAN_SWEEP_TOTAL,
        &*GOSSIP_DUPLICATE_TOTAL,
        &*GOSSIP_FANOUT_GAUGE,
        &*GOSSIP_LATENCY_BUCKETS,
        GOSSIP_PEER_FAILURE_TOTAL
            .ensure_handle_for_label_values(&["__"])
            .expect(LABEL_REGISTRATION_ERR),
        &*SHARD_CACHE_EVICT_TOTAL,
        &*PARTITION_EVENTS_TOTAL,
        &*PARTITION_RECOVER_BLOCKS,
        DEX_ORDERS_TOTAL
            .ensure_handle_for_label_values(&["buy"])
            .expect(LABEL_REGISTRATION_ERR),
        DEX_ORDERS_TOTAL
            .ensure_handle_for_label_values(&["sell"])
            .expect(LABEL_REGISTRATION_ERR),
        &*DEX_TRADE_VOLUME,
        COMPUTE_SLA_VIOLATIONS_TOTAL
            .ensure_handle_for_label_values(&["__"])
            .expect(LABEL_REGISTRATION_ERR),
        COMPUTE_PROVIDER_UPTIME
            .ensure_handle_for_label_values(&["__"])
            .expect(LABEL_REGISTRATION_ERR),
        P2P_REQUEST_LIMIT_HITS_TOTAL
            .ensure_handle_for_label_values(&[""])
            .expect(LABEL_REGISTRATION_ERR),
        PEER_RATE_LIMIT_TOTAL
            .ensure_handle_for_label_values(&["__"])
            .expect(LABEL_REGISTRATION_ERR),
        &*INVALID_SELECTOR_REJECT_TOTAL,
        &*BALANCE_OVERFLOW_REJECT_TOTAL,
        &*DROP_NOT_FOUND_TOTAL,
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&["mempool"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&["storage"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&["p2p"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&["compute"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_EMIT_TOTAL
            .ensure_handle_for_label_values(&["consensus"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&["mempool"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&["storage"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&["p2p"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&["compute"])
            .expect(LABEL_REGISTRATION_ERR),
        LOG_DROP_TOTAL
            .ensure_handle_for_label_values(&["consensus"])
            .expect(LABEL_REGISTRATION_ERR),
    );

    REGISTRY.render()
}

pub fn gather_metrics() -> PyResult<String> {
    Ok(gather())
}

/// Start a minimal HTTP server that exposes the in-house telemetry snapshot.
///
/// The server runs on a background thread and responds to any incoming
/// connection with the current metrics in text format. The bound socket
/// address is returned so callers can discover the chosen port when using
/// an ephemeral one (e.g. `"127.0.0.1:0"`).
///
/// This helper is intentionally lightweight and meant for tests or local
/// demos; production deployments should place a reverse proxy in front of it.
pub struct MetricsServer {
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MetricsServer {
    pub fn shutdown(mut self) {
        use std::sync::atomic::Ordering;
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MetricsServer {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn serve_metrics_with_shutdown(addr: &str) -> PyResult<(String, MetricsServer)> {
    init_wrapper_metrics();
    use std::io::{Read, Write};
    use std::sync::{atomic::AtomicBool, Arc};
    use std::time::Duration;

    let socket_addr = addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| PyError::runtime(e.to_string()))?;
    let listener =
        net::listener::bind_sync("telemetry", "telemetry_listener_bind_failed", socket_addr)
            .map_err(|e| PyError::runtime(e.to_string()))?;
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|e| panic!("nonblocking: {e}"));
    let local = listener
        .local_addr()
        .map_err(|e| PyError::runtime(e.to_string()))?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    let handle = std::thread::spawn(move || {
        use std::io::ErrorKind;
        while !flag.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut _req = [0u8; 512];
                    let _ = stream.read(&mut _req);
                    let body = gather_metrics().unwrap_or_else(|e| e.message().to_string());
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}",
                        runtime::telemetry::TEXT_MIME,
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });
    Ok((
        local.to_string(),
        MetricsServer {
            shutdown,
            handle: Some(handle),
        },
    ))
}

pub fn serve_metrics(addr: &str) -> PyResult<String> {
    let (addr, handle) = serve_metrics_with_shutdown(addr)?;
    std::mem::forget(handle);
    Ok(addr)
}
#[cfg(all(feature = "telemetry", feature = "telemetry-verbose"))]
const VERBOSE: bool = true;
#[cfg(not(all(feature = "telemetry", feature = "telemetry-verbose")))]
const VERBOSE: bool = false;

#[cfg(feature = "telemetry")]
pub fn verbose() -> bool {
    VERBOSE
}
#[cfg(not(feature = "telemetry"))]
pub fn verbose() -> bool {
    false
}
