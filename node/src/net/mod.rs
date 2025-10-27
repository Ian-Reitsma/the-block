pub mod a_star;
pub mod ban_store;
pub mod discovery;
pub mod listener;
mod message;
pub mod peer;
mod peer_metrics_binary;
pub mod peer_metrics_store;
#[cfg(all(feature = "quic", feature = "quinn"))]
pub mod quic;
#[cfg(all(feature = "quic", feature = "quinn"))]
pub mod quic_stats;
#[cfg(feature = "quic")]
pub mod transport_quic;
pub mod uptime;
#[cfg(any(not(feature = "quic"), not(feature = "quinn")))]
pub mod quic {
    use super::peer::HandshakeError;
    use diagnostics::anyhow::Error;

    #[derive(Debug)]
    pub enum ConnectError {
        Handshake(HandshakeError),
        Other(Error),
    }
}
#[cfg(all(feature = "quic", not(feature = "quinn")))]
pub mod quic_stats {
    pub(super) fn snapshot() -> Vec<super::QuicStatsEntry> {
        Vec::new()
    }

    pub(super) fn record_latency(_peer: &[u8; 32], _ms: u64) {}

    pub(super) fn record_handshake_failure(_peer: &[u8; 32]) {}

    pub(super) fn record_endpoint_reuse(_peer: &[u8; 32]) {}

    pub(super) fn record_address(_peer: &[u8; 32], _addr: std::net::SocketAddr) {}

    pub(super) fn record_retransmit(_count: u64) {}

    #[cfg(feature = "telemetry")]
    pub(super) fn peer_label(_peer: Option<[u8; 32]>) -> String {
        "unknown".to_string()
    }

    #[cfg(not(feature = "telemetry"))]
    pub(super) fn peer_label(_peer: Option<[u8; 32]>) -> String {
        String::new()
    }
}
pub mod partition_watch;
pub mod turbine;

#[cfg(feature = "fuzzy")]
pub use message::decode as fuzz_decode_message;

use crate::config::{OverlayBackend, OverlayConfig};
use crate::net::peer::pk_from_addr;
use crate::{
    gossip::relay::{Relay, RelayStatus},
    BlobTx, Blockchain, ShutdownFlag, SignedTransaction,
};
use base64_fp::{decode_standard, encode_standard};
use coding::default_encryptor;
use concurrency::{Bytes, Lazy, MutexExt, OnceCell};
use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::SigningKey;
use diagnostics::anyhow::anyhow;
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::ShardId;
#[cfg(feature = "telemetry")]
use p2p_overlay::OverlayDiagnostics;
use p2p_overlay::{
    InhouseOverlay, InhousePeerId, OverlayResult, OverlayService, PeerEndpoint, StubOverlay,
};
use rand::{OsRng, Rng, RngCore};
use std::collections::{hash_map::Entry, HashMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::Duration;
use sys::paths;

use runtime::fs::watch::{
    RecursiveMode as WatchRecursiveMode, WatchEventKind, Watcher as FsWatcher,
};

#[cfg(all(feature = "quic", feature = "telemetry"))]
use crate::telemetry::{
    sampled_observe, QUIC_BYTES_RECV_TOTAL, QUIC_BYTES_SENT_TOTAL, QUIC_CERT_ROTATION_TOTAL,
    QUIC_CONN_LATENCY_SECONDS, QUIC_DISCONNECT_TOTAL, QUIC_ENDPOINT_REUSE_TOTAL,
    QUIC_HANDSHAKE_FAIL_TOTAL, QUIC_PROVIDER_CONNECT_TOTAL, QUIC_RETRANSMIT_TOTAL,
};
#[cfg(feature = "telemetry")]
use crate::telemetry::{OVERLAY_BACKEND_ACTIVE, OVERLAY_PEER_PERSISTED_TOTAL, OVERLAY_PEER_TOTAL};
#[cfg(feature = "quinn")]
use transport::QuinnDisconnect;
#[cfg(feature = "quic")]
use transport::{
    self, Config as TransportConfig, DefaultFactory as TransportDefaultFactory, ProviderKind,
    ProviderRegistry as TransportProviderRegistry, TransportCallbacks,
};

pub use crate::p2p::handshake::{Hello, Transport, SUPPORTED_VERSION};
pub use message::{BlobChunk, Message, Payload};
pub use peer::ReputationUpdate;
pub use peer::{
    clear_peer_metrics, clear_throttle, export_all_peer_stats, export_peer_stats, known_peers,
    load_peer_metrics, p2p_max_bytes_per_sec, p2p_max_per_sec, peer_reputation_decay, peer_stats,
    peer_stats_all, peer_stats_map, persist_peer_metrics, publish_telemetry_summary,
    recent_handshake_failures, record_request, reset_peer_metrics, rotate_peer_key,
    set_max_peer_metrics, set_metrics_aggregator, set_metrics_export_dir,
    set_p2p_max_bytes_per_sec, set_p2p_max_per_sec, set_peer_metrics_compress,
    set_peer_metrics_export, set_peer_metrics_export_quota, set_peer_metrics_path,
    set_peer_metrics_retention, set_peer_metrics_sample_rate, set_peer_reputation_decay,
    set_track_drop_reasons, set_track_handshake_fail, throttle_peer, DropReason, HandshakeError,
    PeerMetrics, PeerReputation, PeerSet, PeerStat,
};

pub use peer::simulate_handshake_fail;

pub type OverlayPeerId = InhousePeerId;
pub type OverlayAddress = PeerEndpoint;

type DynOverlayService = Arc<dyn OverlayService<Peer = OverlayPeerId, Address = OverlayAddress>>;

static OVERLAY_SERVICE: Lazy<RwLock<DynOverlayService>> = Lazy::new(|| {
    let path = default_overlay_path();
    RwLock::new(build_inhouse_overlay(path))
});

fn read_lock<'a, T>(lock: &'a RwLock<T>) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poison) => {
            diagnostics::tracing::warn!(target: "net", "rwlock_read_poisoned_recovered");
            poison.into_inner()
        }
    }
}

fn write_lock<'a, T>(lock: &'a RwLock<T>) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poison) => {
            diagnostics::tracing::warn!(target: "net", "rwlock_write_poisoned_recovered");
            poison.into_inner()
        }
    }
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poison) => {
            diagnostics::tracing::warn!(target: "net", "mutex_poisoned_recovered");
            poison.into_inner()
        }
    }
}

#[cfg(feature = "telemetry")]
const OVERLAY_BACKENDS: [&str; 2] = ["inhouse", "stub"];

#[cfg(feature = "telemetry")]
fn overlay_metric_value(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(feature = "telemetry")]
fn with_metric_handle<T, E, F, const N: usize>(
    metric: &str,
    labels: [&str; N],
    result: Result<T, E>,
    apply: F,
) where
    F: FnOnce(T),
    E: std::fmt::Display,
{
    match result {
        Ok(handle) => apply(handle),
        Err(err) => diagnostics::log::warn!(
            "metric_label_registration_failed: metric={metric} labels={labels:?} err={err}"
        ),
    }
}

#[cfg(feature = "telemetry")]
fn record_overlay_metrics(snapshot: &OverlayDiagnostics) {
    let active = overlay_metric_value(snapshot.active_peers);
    let persisted = overlay_metric_value(snapshot.persisted_peers);

    for backend in OVERLAY_BACKENDS {
        let is_active = if backend == snapshot.label { 1 } else { 0 };
        with_metric_handle(
            "overlay_backend_active",
            [backend],
            OVERLAY_BACKEND_ACTIVE.ensure_handle_for_label_values(&[backend]),
            |handle| handle.set(is_active),
        );
        with_metric_handle(
            "overlay_peer_total",
            [backend],
            OVERLAY_PEER_TOTAL.ensure_handle_for_label_values(&[backend]),
            |handle| handle.set(if backend == snapshot.label { active } else { 0 }),
        );
        with_metric_handle(
            "overlay_peer_persisted_total",
            [backend],
            OVERLAY_PEER_PERSISTED_TOTAL.ensure_handle_for_label_values(&[backend]),
            |handle| {
                handle.set(if backend == snapshot.label {
                    persisted
                } else {
                    0
                })
            },
        );
    }

    if !OVERLAY_BACKENDS.contains(&snapshot.label) {
        with_metric_handle(
            "overlay_backend_active",
            [snapshot.label],
            OVERLAY_BACKEND_ACTIVE.ensure_handle_for_label_values(&[snapshot.label]),
            |handle| handle.set(1),
        );
        with_metric_handle(
            "overlay_peer_total",
            [snapshot.label],
            OVERLAY_PEER_TOTAL.ensure_handle_for_label_values(&[snapshot.label]),
            |handle| handle.set(active),
        );
        with_metric_handle(
            "overlay_peer_persisted_total",
            [snapshot.label],
            OVERLAY_PEER_PERSISTED_TOTAL.ensure_handle_for_label_values(&[snapshot.label]),
            |handle| handle.set(persisted),
        );
    }
}

#[cfg(feature = "telemetry")]
fn clear_overlay_metrics() {
    for backend in OVERLAY_BACKENDS {
        with_metric_handle(
            "overlay_backend_active",
            [backend],
            OVERLAY_BACKEND_ACTIVE.ensure_handle_for_label_values(&[backend]),
            |handle| handle.set(0),
        );
        with_metric_handle(
            "overlay_peer_total",
            [backend],
            OVERLAY_PEER_TOTAL.ensure_handle_for_label_values(&[backend]),
            |handle| handle.set(0),
        );
        with_metric_handle(
            "overlay_peer_persisted_total",
            [backend],
            OVERLAY_PEER_PERSISTED_TOTAL.ensure_handle_for_label_values(&[backend]),
            |handle| handle.set(0),
        );
    }
}

#[cfg(feature = "telemetry")]
fn update_overlay_metrics() {
    let service = overlay_service();
    match service.diagnostics() {
        Ok(snapshot) => record_overlay_metrics(&snapshot),
        Err(_) => clear_overlay_metrics(),
    }
}

#[cfg(not(feature = "telemetry"))]
fn update_overlay_metrics() {}

fn build_inhouse_overlay(path: PathBuf) -> DynOverlayService {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    #[cfg(feature = "telemetry")]
    let overlay = InhouseOverlay::with_metrics(path.clone(), uptime::Metrics);
    #[cfg(not(feature = "telemetry"))]
    let overlay = InhouseOverlay::new(path.clone());
    Arc::new(overlay)
}

fn build_stub_overlay() -> DynOverlayService {
    #[cfg(feature = "telemetry")]
    let overlay = StubOverlay::with_metrics(uptime::Metrics);
    #[cfg(not(feature = "telemetry"))]
    let overlay = StubOverlay::new();
    Arc::new(overlay)
}

#[derive(Clone, Debug, Serialize)]
pub struct OverlayStatus {
    pub backend: String,
    pub active_peers: usize,
    pub persisted_peers: usize,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub database_path: Option<String>,
}

fn default_overlay_path() -> PathBuf {
    std::env::var("TB_OVERLAY_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("overlay")
                .join("peers.json")
        })
}

pub fn install_overlay(service: DynOverlayService) {
    {
        *write_lock(&*OVERLAY_SERVICE) = service;
    }
    update_overlay_metrics();
}

pub fn overlay_service() -> DynOverlayService {
    read_lock(&*OVERLAY_SERVICE).clone()
}

pub fn configure_overlay(cfg: &OverlayConfig) {
    match cfg.backend {
        OverlayBackend::Inhouse => {
            let path = PathBuf::from(&cfg.peer_db_path);
            install_overlay(build_inhouse_overlay(path));
        }
        OverlayBackend::Stub => {
            install_overlay(build_stub_overlay());
        }
    }
}

pub fn overlay_status() -> OverlayStatus {
    let service = overlay_service();
    match service.diagnostics() {
        Ok(snapshot) => {
            #[cfg(feature = "telemetry")]
            record_overlay_metrics(&snapshot);
            OverlayStatus {
                backend: snapshot.label.to_string(),
                active_peers: snapshot.active_peers,
                persisted_peers: snapshot.persisted_peers,
                database_path: snapshot
                    .database_path
                    .map(|path| path.to_string_lossy().into_owned()),
            }
        }
        Err(err) => {
            #[cfg(feature = "telemetry")]
            {
                clear_overlay_metrics();
                diagnostics::tracing::warn!(reason = %err, "overlay_diagnostics_failed");
            }
            #[cfg(not(feature = "telemetry"))]
            eprintln!("overlay_diagnostics_failed: {err}");
            OverlayStatus {
                backend: "unknown".into(),
                active_peers: 0,
                persisted_peers: 0,
                database_path: None,
            }
        }
    }
}

pub fn overlay_peer_from_bytes(bytes: &[u8]) -> OverlayResult<OverlayPeerId> {
    overlay_service().peer_from_bytes(bytes)
}

pub fn overlay_peer_to_bytes(peer: &OverlayPeerId) -> Vec<u8> {
    overlay_service().peer_to_bytes(peer)
}

pub fn overlay_peer_from_base58(value: &str) -> OverlayResult<OverlayPeerId> {
    InhousePeerId::from_base58(value)
}

pub fn overlay_peer_to_base58(peer: &OverlayPeerId) -> String {
    peer.to_base58()
}

#[cfg(all(feature = "quic", feature = "quinn"))]
pub fn quic_stats() -> Vec<QuicStatsEntry> {
    quic_stats::snapshot()
}

#[cfg(any(not(feature = "quic"), not(feature = "quinn")))]
pub fn quic_stats() -> Vec<QuicStatsEntry> {
    Vec::new()
}

#[cfg(feature = "quic")]
static TRANSPORT_FACTORY: Lazy<RwLock<Arc<dyn transport::TransportFactory>>> =
    Lazy::new(|| RwLock::new(Arc::new(TransportDefaultFactory::default()) as Arc<_>));

#[cfg(feature = "quic")]
static TRANSPORT_REGISTRY: Lazy<RwLock<Option<TransportProviderRegistry>>> =
    Lazy::new(|| RwLock::new(None));

#[cfg(feature = "quic")]
pub fn install_transport_factory(factory: Arc<dyn transport::TransportFactory>) {
    *write_lock(&*TRANSPORT_FACTORY) = factory;
}

#[cfg(feature = "quic")]
pub fn configure_transport(cfg: &TransportConfig) -> diagnostics::anyhow::Result<()> {
    let callbacks = build_transport_callbacks();
    let factory = read_lock(&*TRANSPORT_FACTORY).clone();
    #[cfg(all(feature = "quic", feature = "inhouse"))]
    {
        let override_path = if cfg.provider == ProviderKind::Inhouse {
            cfg.certificate_cache.clone()
        } else {
            None
        };
        transport_quic::set_inhouse_cert_store_override(override_path);
    }
    let registry = factory.create(cfg, &callbacks)?;
    *write_lock(&*TRANSPORT_REGISTRY) = Some(registry);
    #[cfg(feature = "telemetry")]
    crate::telemetry::record_transport_backend(cfg.provider.id());
    Ok(())
}

#[cfg(feature = "quic")]
pub(crate) fn transport_registry() -> Option<TransportProviderRegistry> {
    {
        let guard = read_lock(&*TRANSPORT_REGISTRY);
        if guard.is_some() {
            return guard.clone();
        }
    }
    if let Err(err) = configure_transport(&TransportConfig::default()) {
        #[cfg(feature = "telemetry")]
        diagnostics::tracing::warn!(reason = %err, "transport_configure_default_failed");
        #[cfg(not(feature = "telemetry"))]
        eprintln!("transport_configure_default_failed: {err}");
    }
    read_lock(&*TRANSPORT_REGISTRY).clone()
}

#[cfg(feature = "quic")]
fn build_transport_callbacks() -> TransportCallbacks {
    let mut callbacks = TransportCallbacks::default();

    #[cfg(feature = "quic")]
    #[allow(unused_variables)]
    let provider_counter: Arc<dyn Fn(&'static str) + Send + Sync + 'static> = {
        let cb = Arc::new(|provider: &'static str| {
            #[cfg(feature = "telemetry")]
            {
                with_metric_handle(
                    "transport_provider_connect_total",
                    [provider],
                    crate::telemetry::TRANSPORT_PROVIDER_CONNECT_TOTAL
                        .ensure_handle_for_label_values(&[provider]),
                    |handle| handle.inc(),
                );
                with_metric_handle(
                    "quic_provider_connect_total",
                    [provider],
                    QUIC_PROVIDER_CONNECT_TOTAL.ensure_handle_for_label_values(&[provider]),
                    |handle| handle.inc(),
                );
            }
            #[cfg(not(feature = "telemetry"))]
            let _ = provider;
        });
        cb
    };

    #[cfg(feature = "quinn")]
    {
        let quinn = &mut callbacks.quinn;
        quinn.provider_connect = Some(provider_counter.clone());
        quinn.handshake_latency = Some(Arc::new(|addr: SocketAddr, elapsed: Duration| {
            #[cfg(feature = "telemetry")]
            sampled_observe(&QUIC_CONN_LATENCY_SECONDS, elapsed.as_secs_f64());
            if let Some(pk) = pk_from_addr(&addr) {
                peer::record_handshake_latency(&pk, elapsed.as_millis() as u64);
            }
        }));

        quinn.handshake_failure = Some(Arc::new(|addr: SocketAddr, err| {
            let mapped = map_quinn_handshake_error(err);
            if let Some(pk) = pk_from_addr(&addr) {
                quic_stats::record_handshake_failure(&pk);
            }
            peer::record_handshake_fail_addr(addr, mapped);
            #[cfg(feature = "telemetry")]
            {
                if peer::track_handshake_fail_enabled() {
                    let peer_label = quic_stats::peer_label(pk_from_addr(&addr));
                    with_metric_handle(
                        "quic_handshake_fail_total",
                        [peer_label.as_str(), mapped.as_str()],
                        QUIC_HANDSHAKE_FAIL_TOTAL.ensure_handle_for_label_values(&[
                            peer_label.as_str(),
                            mapped.as_str(),
                        ]),
                        |handle| handle.inc(),
                    );
                }
                diagnostics::tracing::error!(reason = mapped.as_str(), "quic_connect_fail");
            }
        }));

        quinn.endpoint_reuse = Some(Arc::new(|addr: SocketAddr| {
            #[cfg(feature = "telemetry")]
            QUIC_ENDPOINT_REUSE_TOTAL.inc();
            if let Some(pk) = pk_from_addr(&addr) {
                quic_stats::record_endpoint_reuse(&pk);
            }
        }));

        quinn.bytes_sent = Some(Arc::new(|_addr: SocketAddr, bytes: u64| {
            #[cfg(feature = "telemetry")]
            QUIC_BYTES_SENT_TOTAL.inc_by(bytes);
            #[cfg(not(feature = "telemetry"))]
            let _ = bytes;
        }));

        quinn.bytes_received = Some(Arc::new(|_addr: SocketAddr, bytes: u64| {
            #[cfg(feature = "telemetry")]
            QUIC_BYTES_RECV_TOTAL.inc_by(bytes);
            #[cfg(not(feature = "telemetry"))]
            let _ = bytes;
        }));

        quinn.disconnect = Some(Arc::new(|_addr: SocketAddr, reason: QuinnDisconnect| {
            #[cfg(feature = "telemetry")]
            {
                let label = reason.label();
                with_metric_handle(
                    "quic_disconnect_total",
                    [label.as_ref()],
                    QUIC_DISCONNECT_TOTAL.ensure_handle_for_label_values(&[label.as_ref()]),
                    |handle| handle.inc(),
                );
            }
            #[cfg(not(feature = "telemetry"))]
            let _ = reason;
        }));
    }

    {
        let s2n = &mut callbacks.s2n;
        #[cfg(feature = "s2n-quic")]
        {
            s2n.provider_connect = Some(provider_counter.clone());
        }
        s2n.cert_rotated = Some(Arc::new(|label: &'static str| {
            #[cfg(feature = "telemetry")]
            with_metric_handle(
                "quic_cert_rotation_total",
                [label],
                QUIC_CERT_ROTATION_TOTAL.ensure_handle_for_label_values(&[label]),
                |handle| handle.inc(),
            );
            #[cfg(not(feature = "telemetry"))]
            let _ = label;
        }));

        s2n.handshake_failure = Some(Arc::new(|reason: &str| {
            #[cfg(feature = "telemetry")]
            {
                let peer_label = quic_stats::peer_label(None);
                with_metric_handle(
                    "quic_handshake_fail_total",
                    [peer_label.as_str(), reason],
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&[peer_label.as_str(), reason]),
                    |handle| handle.inc(),
                );
            }
            #[cfg(not(feature = "telemetry"))]
            let _ = reason;
        }));

        s2n.retransmit = Some(Arc::new(|count: u64| {
            #[cfg(feature = "telemetry")]
            QUIC_RETRANSMIT_TOTAL.inc_by(count);
            #[cfg(not(feature = "telemetry"))]
            let _ = count;
        }));
    }

    #[cfg(feature = "inhouse")]
    {
        let inhouse = &mut callbacks.inhouse;
        inhouse.provider_connect = Some(provider_counter.clone());
        inhouse.handshake_success = Some(Arc::new(|addr: SocketAddr| {
            if let Some(pk) = pk_from_addr(&addr) {
                quic_stats::record_address(&pk, addr);
            }
        }));
        inhouse.handshake_failure = Some(Arc::new(|addr: SocketAddr, reason: &str| {
            if let Some(pk) = pk_from_addr(&addr) {
                quic_stats::record_handshake_failure(&pk);
            }
            #[cfg(feature = "telemetry")]
            {
                let peer_label = quic_stats::peer_label(pk_from_addr(&addr));
                with_metric_handle(
                    "quic_handshake_fail_total",
                    [peer_label.as_str(), reason],
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&[peer_label.as_str(), reason]),
                    |handle| handle.inc(),
                );
            }
        }));
    }

    callbacks
}

#[cfg(feature = "quic")]
fn map_quinn_handshake_error(err: transport::HandshakeError) -> peer::HandshakeError {
    match err {
        transport::HandshakeError::Tls => peer::HandshakeError::Tls,
        transport::HandshakeError::Version => peer::HandshakeError::Version,
        transport::HandshakeError::Timeout => peer::HandshakeError::Timeout,
        transport::HandshakeError::Certificate => peer::HandshakeError::Certificate,
        transport::HandshakeError::Other => peer::HandshakeError::Other,
    }
}

#[cfg(feature = "quic")]
fn capability_label(cap: transport::ProviderCapability) -> &'static str {
    match cap {
        transport::ProviderCapability::CertificateRotation => "certificate_rotation",
        transport::ProviderCapability::ConnectionPooling => "connection_pooling",
        transport::ProviderCapability::InsecureConnect => "insecure_connect",
        transport::ProviderCapability::TelemetryCallbacks => "telemetry_callbacks",
    }
}

const PEER_CERT_STORE_FILE: &str = "quic_peer_certs.json";
const CERT_BLOB_PREFIX: &str = "enc:v1:";
const DISABLE_PERSIST_ENV: &str = "TB_PEER_CERT_DISABLE_DISK";

#[derive(Clone)]
struct CertSnapshot {
    fingerprint: [u8; 32],
    cert: Bytes,
    updated_at: u64,
}

#[derive(Clone)]
struct PeerCertStore {
    current: CertSnapshot,
    history: VecDeque<CertSnapshot>,
    rotations: u64,
}

type ProviderCertStores = HashMap<String, PeerCertStore>;

#[derive(Clone, Serialize, Deserialize)]
struct CertDiskRecord {
    fingerprint: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    cert: Option<String>,
    updated_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct PeerCertDiskEntry {
    peer: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    providers: Vec<ProviderDiskRecord>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    current: Option<CertDiskRecord>,
    history: Vec<CertDiskRecord>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    rotations: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ProviderDiskRecord {
    provider: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    current: Option<CertDiskRecord>,
    #[serde(default = "foundation_serialization::defaults::default")]
    history: Vec<CertDiskRecord>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    rotations: Option<u64>,
}

static PEER_CERTS: Lazy<RwLock<HashMap<[u8; 32], ProviderCertStores>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static PEER_CERTS_INITIALIZED: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
static PEER_CERT_HISTORY_LIMIT: Lazy<AtomicUsize> =
    Lazy::new(|| AtomicUsize::new(DEFAULT_PEER_CERT_HISTORY));
static PEER_CERT_MAX_AGE: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(DEFAULT_PEER_CERT_MAX_AGE));
static PEER_CERT_WATCH_PATH: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));
static PEER_CERT_PERSIST_DISABLED: Lazy<bool> =
    Lazy::new(|| std::env::var_os(DISABLE_PERSIST_ENV).is_some());
static PEER_CERT_ENC_KEY: OnceCell<Option<[u8; 32]>> = OnceCell::new();
static GOSSIP_RELAY: Lazy<RwLock<Option<std::sync::Arc<Relay>>>> = Lazy::new(|| RwLock::new(None));

pub fn set_gossip_relay(relay: std::sync::Arc<Relay>) {
    *write_lock(&*GOSSIP_RELAY) = Some(relay);
}

/// Restores the previous gossip relay when dropped.
pub struct GossipRelayGuard {
    previous: Option<std::sync::Arc<Relay>>,
}

impl Drop for GossipRelayGuard {
    fn drop(&mut self) {
        let mut slot = write_lock(&*GOSSIP_RELAY);
        *slot = self.previous.take();
    }
}

/// Install a temporary gossip relay, restoring the prior instance when the guard is dropped.
pub fn scoped_gossip_relay(relay: std::sync::Arc<Relay>) -> GossipRelayGuard {
    let mut slot = write_lock(&*GOSSIP_RELAY);
    let previous = slot.replace(relay);
    GossipRelayGuard { previous }
}

pub fn gossip_status() -> Option<RelayStatus> {
    read_lock(&*GOSSIP_RELAY)
        .as_ref()
        .map(|relay| relay.status())
}

pub fn gossip_selected_peers() -> Option<Vec<String>> {
    gossip_status().and_then(|status| status.fanout.selected_peers)
}

pub fn register_shard_peer(shard: ShardId, peer: OverlayPeerId) {
    if let Some(relay) = read_lock(&*GOSSIP_RELAY).as_ref() {
        relay.register_peer(shard, peer);
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct QuicStatsEntry {
    pub peer_id: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub provider: Option<String>,
    pub retransmits: u64,
    pub endpoint_reuse: u64,
    pub handshake_failures: u64,
    pub last_updated: u64,
}

#[derive(Clone, Serialize)]
pub struct PeerCertSnapshot {
    pub peer: [u8; 32],
    pub provider: String,
    pub fingerprint: [u8; 32],
    pub updated_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerCertHistoryEntry {
    pub peer: String,
    pub provider: String,
    pub current: PeerCertHistoryRecord,
    pub history: Vec<PeerCertHistoryRecord>,
    pub rotations: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerCertHistoryRecord {
    pub fingerprint: String,
    pub updated_at: u64,
    pub age_secs: u64,
    pub has_certificate: bool,
}

const DEFAULT_PEER_CERT_HISTORY: usize = 4;
const DEFAULT_PEER_CERT_MAX_AGE: u64 = 30 * 24 * 60 * 60;

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn peer_cert_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("TB_PEER_CERT_CACHE_PATH") {
        return PathBuf::from(path);
    }
    paths::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".the_block")
        .join(PEER_CERT_STORE_FILE)
}

fn peer_cert_persistence_enabled() -> bool {
    !*PEER_CERT_PERSIST_DISABLED
}

fn max_peer_cert_history() -> usize {
    PEER_CERT_HISTORY_LIMIT.load(Ordering::Relaxed)
}

fn max_peer_cert_age_secs() -> u64 {
    PEER_CERT_MAX_AGE.load(Ordering::Relaxed)
}

#[cfg(feature = "quic")]
fn default_legacy_provider_id() -> String {
    if cfg!(feature = "s2n-quic") {
        transport::ProviderKind::S2nQuic.id().to_string()
    } else {
        transport::ProviderKind::Quinn.id().to_string()
    }
}

#[cfg(not(feature = "quic"))]
fn default_legacy_provider_id() -> String {
    "quinn".to_string()
}

pub fn configure_peer_cert_policy(history: Option<usize>, max_age: Option<u64>) {
    let history = history.unwrap_or(DEFAULT_PEER_CERT_HISTORY);
    let max_age = max_age.unwrap_or(DEFAULT_PEER_CERT_MAX_AGE);
    PEER_CERT_HISTORY_LIMIT.store(history, Ordering::Relaxed);
    PEER_CERT_MAX_AGE.store(max_age, Ordering::Relaxed);
}

fn peer_cert_encryption_key() -> Option<[u8; 32]> {
    if !peer_cert_persistence_enabled() {
        return None;
    }
    PEER_CERT_ENC_KEY
        .get_or_init(|| {
            if let Ok(key_hex) = std::env::var("TB_PEER_CERT_KEY_HEX") {
                if let Ok(bytes) = crypto_suite::hex::decode(key_hex.trim()) {
                    if bytes.len() == 32 {
                        let mut key = [0u8; 32];
                        key.copy_from_slice(&bytes);
                        return Some(key);
                    }
                    let hash = blake3::hash(&bytes);
                    let mut key = [0u8; 32];
                    key.copy_from_slice(hash.as_bytes());
                    return Some(key);
                }
            }
            if let Ok(node_hex) = std::env::var("TB_NODE_KEY_HEX") {
                if let Ok(bytes) = crypto_suite::hex::decode(node_hex.trim()) {
                    let hash = blake3::hash(&bytes);
                    let mut key = [0u8; 32];
                    key.copy_from_slice(hash.as_bytes());
                    return Some(key);
                }
            }
            let key = load_net_key();
            Some(blake3::derive_key(
                "the-block:quic-peer-cert-store",
                &key.to_keypair_bytes(),
            ))
        })
        .clone()
}

fn encrypt_cert_blob(cert: &[u8]) -> Option<String> {
    if cert.is_empty() {
        return None;
    }
    if let Some(key) = peer_cert_encryption_key() {
        if let Ok(encryptor) = default_encryptor(&key) {
            if let Ok(ciphertext) = encryptor.encrypt(cert) {
                return Some(format!(
                    "{}{}",
                    CERT_BLOB_PREFIX,
                    encode_standard(&ciphertext)
                ));
            }
        }
    }
    Some(encode_standard(cert))
}

fn decrypt_cert_blob(data: &str) -> Option<Vec<u8>> {
    if data.starts_with(CERT_BLOB_PREFIX) {
        let payload = decode_standard(&data[CERT_BLOB_PREFIX.len()..]).ok()?;
        let key = peer_cert_encryption_key()?;
        let encryptor = default_encryptor(&key).ok()?;
        encryptor.decrypt(&payload).ok()
    } else {
        decode_standard(data).ok()
    }
}

fn prune_store_entry(store: &mut PeerCertStore, now: u64) {
    store
        .history
        .retain(|snapshot| now.saturating_sub(snapshot.updated_at) <= max_peer_cert_age_secs());
    while store.history.len() > max_peer_cert_history() {
        store.history.pop_back();
    }
}

fn reload_peer_cert_store_from_path(path: &Path) -> bool {
    if !peer_cert_persistence_enabled() {
        return false;
    }
    let now = unix_now();
    match fs::read(path) {
        Ok(data) => match json::from_slice::<Vec<PeerCertDiskEntry>>(&data) {
            Ok(entries) => {
                let mut rebuilt = HashMap::new();
                let legacy_provider = default_legacy_provider_id();
                for entry in entries {
                    if let Ok(bytes) = crypto_suite::hex::decode(&entry.peer) {
                        if bytes.len() != 32 {
                            continue;
                        }
                        let mut peer = [0u8; 32];
                        peer.copy_from_slice(&bytes);
                        let mut stores = HashMap::new();
                        let provider_records = if entry.providers.is_empty() {
                            vec![ProviderDiskRecord {
                                provider: legacy_provider.clone(),
                                current: entry.current.clone(),
                                history: entry.history.clone(),
                                rotations: entry.rotations,
                            }]
                        } else {
                            entry.providers
                        };
                        for provider_entry in provider_records {
                            if let Some(current) = provider_entry
                                .current
                                .as_ref()
                                .and_then(|rec| disk_to_snapshot(rec))
                            {
                                let mut history = VecDeque::new();
                                for rec in &provider_entry.history {
                                    if let Some(snapshot) = disk_to_snapshot(rec) {
                                        history.push_back(snapshot);
                                    }
                                }
                                let mut store = PeerCertStore {
                                    current,
                                    history,
                                    rotations: provider_entry.rotations.unwrap_or(0),
                                };
                                prune_store_entry(&mut store, now);
                                stores.insert(provider_entry.provider.clone(), store);
                            }
                        }
                        if !stores.is_empty() {
                            rebuilt.insert(peer, stores);
                        }
                    }
                }
                let mut map = write_lock(&*PEER_CERTS);
                *map = rebuilt;
                true
            }
            Err(_) => false,
        },
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                let mut map = write_lock(&*PEER_CERTS);
                map.clear();
                true
            } else {
                false
            }
        }
    }
}

fn reload_peer_cert_store_from_disk() -> bool {
    let path = peer_cert_store_path();
    reload_peer_cert_store_from_path(&path)
}

fn spawn_peer_cert_store_watch(path: PathBuf) {
    if !peer_cert_persistence_enabled() {
        return;
    }
    let mut guard = lock_mutex(&*PEER_CERT_WATCH_PATH);
    if guard.as_ref() == Some(&path) {
        return;
    }
    *guard = Some(path.clone());
    runtime::spawn(async move {
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        match FsWatcher::new(&parent, WatchRecursiveMode::NonRecursive) {
            Ok(mut watcher) => loop {
                match watcher.next().await {
                    Ok(event)
                        if matches!(
                            event.kind,
                            WatchEventKind::Created
                                | WatchEventKind::Modified
                                | WatchEventKind::Removed
                        ) =>
                    {
                        if event
                            .paths
                            .iter()
                            .any(|changed| is_relevant_change(changed, &path))
                        {
                            let _ = reload_peer_cert_store_from_disk();
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        diagnostics::log::warn!("peer_cert_store_watch_error: {err}");
                        runtime::sleep(Duration::from_secs(1)).await;
                    }
                }
            },
            Err(err) => {
                diagnostics::log::warn!("peer_cert_store_watch_init_failed: {err}");
            }
        }
    });
}

fn is_relevant_change(changed: &Path, target: &Path) -> bool {
    if changed == target {
        return true;
    }
    match (changed.file_name(), target.file_name()) {
        (Some(changed_name), Some(target_name)) => changed_name == target_name,
        _ => false,
    }
}

fn ensure_peer_cert_store_loaded() {
    if PEER_CERTS_INITIALIZED.load(Ordering::SeqCst) {
        return;
    }
    if !peer_cert_persistence_enabled() {
        PEER_CERTS_INITIALIZED.store(true, Ordering::SeqCst);
        return;
    }
    let path = peer_cert_store_path();
    let _ = reload_peer_cert_store_from_path(&path);
    spawn_peer_cert_store_watch(path);
    PEER_CERTS_INITIALIZED.store(true, Ordering::SeqCst);
}

fn sorted_peer_provider_entries<'a>(
    map: &'a HashMap<[u8; 32], ProviderCertStores>,
) -> Vec<([u8; 32], Vec<(&'a String, &'a PeerCertStore)>)> {
    let mut peers: Vec<_> = map.iter().collect();
    peers.sort_by(|(a, _), (b, _)| a.cmp(b));
    peers
        .into_iter()
        .map(|(peer, providers)| {
            let mut provider_entries: Vec<_> = providers.iter().collect();
            provider_entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            (*peer, provider_entries)
        })
        .collect()
}

fn disk_entries_from_map(map: &HashMap<[u8; 32], ProviderCertStores>) -> Vec<PeerCertDiskEntry> {
    sorted_peer_provider_entries(map)
        .into_iter()
        .map(|(peer, providers)| {
            let provider_records: Vec<ProviderDiskRecord> = providers
                .into_iter()
                .map(|(provider, store)| ProviderDiskRecord {
                    provider: provider.clone(),
                    current: Some(snapshot_to_disk(&store.current)),
                    history: store.history.iter().map(snapshot_to_disk).collect(),
                    rotations: Some(store.rotations),
                })
                .collect();
            PeerCertDiskEntry {
                peer: crypto_suite::hex::encode(peer),
                providers: provider_records,
                current: None,
                history: Vec::new(),
                rotations: None,
            }
        })
        .collect()
}

fn persist_peer_cert_store(map: &mut HashMap<[u8; 32], ProviderCertStores>) {
    if !peer_cert_persistence_enabled() {
        return;
    }
    let path = peer_cert_store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = unix_now();
    for stores in map.values_mut() {
        for store in stores.values_mut() {
            prune_store_entry(store, now);
        }
    }
    let entries = disk_entries_from_map(map);
    if let Ok(json) = json::to_vec_pretty(&entries) {
        let _ = fs::write(path, json);
    }
}

fn disk_to_snapshot(record: &CertDiskRecord) -> Option<CertSnapshot> {
    let fingerprint_bytes = crypto_suite::hex::decode(&record.fingerprint).ok()?;
    if fingerprint_bytes.len() != 32 {
        return None;
    }
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&fingerprint_bytes);
    let cert_vec = record
        .cert
        .as_ref()
        .and_then(|c| decrypt_cert_blob(c))
        .unwrap_or_default();
    if !cert_vec.is_empty() {
        let hash = blake3::hash(&cert_vec);
        if hash.as_bytes() != &fingerprint {
            return None;
        }
    }
    Some(CertSnapshot {
        fingerprint,
        cert: Bytes::from(cert_vec),
        updated_at: record.updated_at,
    })
}

fn snapshot_to_disk(snapshot: &CertSnapshot) -> CertDiskRecord {
    CertDiskRecord {
        fingerprint: crypto_suite::hex::encode(snapshot.fingerprint),
        cert: encrypt_cert_blob(snapshot.cert.as_ref()),
        updated_at: snapshot.updated_at,
    }
}

fn append_previous(entry: &mut PeerCertStore, previous: &[[u8; 32]], now: u64) {
    for fp in previous {
        if entry.current.fingerprint == *fp || entry.history.iter().any(|h| h.fingerprint == *fp) {
            continue;
        }
        entry.history.push_back(CertSnapshot {
            fingerprint: *fp,
            cert: Bytes::new(),
            updated_at: now,
        });
    }
}

pub fn record_peer_certificate(
    peer: &[u8; 32],
    provider: &str,
    cert: Bytes,
    fingerprint: [u8; 32],
    previous: Vec<[u8; 32]>,
) {
    if !cert.is_empty() {
        let computed = blake3::hash(cert.as_ref());
        if computed.as_bytes() != &fingerprint {
            return;
        }
    }
    ensure_peer_cert_store_loaded();
    let mut map = write_lock(&*PEER_CERTS);
    let now = unix_now();
    let key = provider.to_string();
    match map.entry(*peer) {
        Entry::Vacant(slot) => {
            let mut store = PeerCertStore {
                current: CertSnapshot {
                    fingerprint,
                    cert: cert.clone(),
                    updated_at: now,
                },
                history: VecDeque::new(),
                rotations: 0,
            };
            append_previous(&mut store, &previous, now);
            prune_store_entry(&mut store, now);
            let mut providers = HashMap::new();
            providers.insert(key, store);
            slot.insert(providers);
        }
        Entry::Occupied(mut entry) => {
            let providers = entry.get_mut();
            let store = providers.entry(key).or_insert_with(|| PeerCertStore {
                current: CertSnapshot {
                    fingerprint,
                    cert: cert.clone(),
                    updated_at: now,
                },
                history: VecDeque::new(),
                rotations: 0,
            });
            if store.current.fingerprint != fingerprint {
                let prev = CertSnapshot {
                    fingerprint: store.current.fingerprint,
                    cert: std::mem::take(&mut store.current.cert),
                    updated_at: store.current.updated_at,
                };
                store.history.push_front(prev);
                store.current = CertSnapshot {
                    fingerprint,
                    cert: cert.clone(),
                    updated_at: now,
                };
                store.rotations = store.rotations.saturating_add(1);
                append_previous(store, &previous, now);
                #[cfg(feature = "telemetry")]
                {
                    let label = crypto_suite::hex::encode(peer);
                    with_metric_handle(
                        "quic_cert_rotation_total",
                        [label.as_str()],
                        crate::telemetry::QUIC_CERT_ROTATION_TOTAL
                            .ensure_handle_for_label_values(&[label.as_str()]),
                        |handle| handle.inc(),
                    );
                }
            } else {
                store.current.cert = cert.clone();
                store.current.updated_at = now;
            }
            prune_store_entry(store, now);
        }
    }
    persist_peer_cert_store(&mut map);
}

fn verify_peer_fingerprint_inner(
    map: &HashMap<[u8; 32], ProviderCertStores>,
    peer: &[u8; 32],
    provider: Option<&str>,
    fingerprint: Option<&[u8; 32]>,
) -> bool {
    let stores = match map.get(peer) {
        Some(stores) => stores,
        None => return fingerprint.is_none(),
    };
    if let Some(fp) = fingerprint {
        if let Some(id) = provider {
            return stores
                .get(id)
                .map(|store| {
                    if &store.current.fingerprint == fp {
                        true
                    } else {
                        store.history.iter().any(|h| &h.fingerprint == fp)
                    }
                })
                .unwrap_or(false);
        }
        for store in stores.values() {
            if &store.current.fingerprint == fp
                || store.history.iter().any(|h| &h.fingerprint == fp)
            {
                return true;
            }
        }
        false
    } else {
        false
    }
}

pub fn verify_peer_fingerprint(peer: &[u8; 32], fingerprint: Option<&[u8; 32]>) -> bool {
    ensure_peer_cert_store_loaded();
    {
        let map = read_lock(&*PEER_CERTS);
        let result = verify_peer_fingerprint_inner(&map, peer, None, fingerprint);
        if result || fingerprint.is_none() {
            return result;
        }
    }
    if reload_peer_cert_store_from_disk() {
        let map = read_lock(&*PEER_CERTS);
        return verify_peer_fingerprint_inner(&map, peer, None, fingerprint);
    }
    false
}

pub fn peer_cert_snapshot() -> Vec<PeerCertSnapshot> {
    ensure_peer_cert_store_loaded();
    let map = read_lock(&*PEER_CERTS);
    let mut snapshots = Vec::new();
    for (peer, providers) in sorted_peer_provider_entries(&map) {
        for (provider, store) in providers {
            snapshots.push(PeerCertSnapshot {
                peer,
                provider: provider.clone(),
                fingerprint: store.current.fingerprint,
                updated_at: store.current.updated_at,
            });
        }
    }
    snapshots
}

fn history_record(snapshot: &CertSnapshot, now: u64) -> PeerCertHistoryRecord {
    PeerCertHistoryRecord {
        fingerprint: crypto_suite::hex::encode(snapshot.fingerprint),
        updated_at: snapshot.updated_at,
        age_secs: now.saturating_sub(snapshot.updated_at),
        has_certificate: !snapshot.cert.is_empty(),
    }
}

pub fn peer_cert_history() -> Vec<PeerCertHistoryEntry> {
    ensure_peer_cert_store_loaded();
    let now = unix_now();
    let map = read_lock(&*PEER_CERTS);
    let mut entries: Vec<_> = Vec::new();
    for (peer, providers) in sorted_peer_provider_entries(&map) {
        for (provider, store) in providers {
            entries.push(PeerCertHistoryEntry {
                peer: crypto_suite::hex::encode(peer),
                provider: provider.clone(),
                current: history_record(&store.current, now),
                history: store
                    .history
                    .iter()
                    .map(|h| history_record(h, now))
                    .collect(),
                rotations: store.rotations,
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::{
        disk_entries_from_map, sorted_peer_provider_entries, CertSnapshot, PeerCertStore,
        ProviderCertStores,
    };
    use concurrency::Bytes;
    use std::collections::{HashMap, VecDeque};

    fn make_store(current: u8, history: &[(u8, u64)], rotations: u64) -> PeerCertStore {
        let current_snapshot = CertSnapshot {
            fingerprint: [current; 32],
            cert: Bytes::from_static(b"cert"),
            updated_at: 100,
        };
        let history_entries: Vec<_> = history
            .iter()
            .map(|(fp, ts)| CertSnapshot {
                fingerprint: [*fp; 32],
                cert: Bytes::new(),
                updated_at: *ts,
            })
            .collect();
        PeerCertStore {
            current: current_snapshot,
            history: VecDeque::from(history_entries),
            rotations,
        }
    }

    #[test]
    fn sorted_peer_entries_order_peers_and_providers() {
        let mut first = ProviderCertStores::new();
        first.insert("beta".to_string(), make_store(2, &[], 0));
        first.insert("alpha".to_string(), make_store(1, &[], 0));

        let mut second = ProviderCertStores::new();
        second.insert("gamma".to_string(), make_store(4, &[], 0));
        second.insert("delta".to_string(), make_store(3, &[], 0));

        let mut map = HashMap::new();
        map.insert([9u8; 32], first);
        map.insert([1u8; 32], second);

        let entries = sorted_peer_provider_entries(&map);
        let peer_hex: Vec<String> = entries
            .iter()
            .map(|(peer, _)| crypto_suite::hex::encode(peer))
            .collect();
        assert_eq!(
            peer_hex,
            vec![
                crypto_suite::hex::encode([1u8; 32]),
                crypto_suite::hex::encode([9u8; 32])
            ]
        );

        let providers_first: Vec<&str> = entries[0].1.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(providers_first, vec!["delta", "gamma"]);

        let providers_second: Vec<&str> = entries[1].1.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(providers_second, vec!["alpha", "beta"]);
    }

    #[test]
    fn disk_entries_preserve_history_and_rotations() {
        let mut providers = ProviderCertStores::new();
        let history = [(7u8, 50u64), (6u8, 40u64), (5u8, 30u64)];
        providers.insert("alpha".to_string(), make_store(9, &history, 12));

        let mut map = HashMap::new();
        map.insert([3u8; 32], providers);

        let entries = disk_entries_from_map(&map);
        assert_eq!(entries.len(), 1);
        let peer_entry = &entries[0];
        assert_eq!(peer_entry.peer, crypto_suite::hex::encode([3u8; 32]));
        assert_eq!(peer_entry.providers.len(), 1);
        let provider = &peer_entry.providers[0];
        assert_eq!(provider.provider, "alpha");
        assert_eq!(provider.rotations, Some(12));
        assert_eq!(provider.history.len(), history.len());
        let recorded_updates: Vec<u64> = provider.history.iter().map(|h| h.updated_at).collect();
        assert_eq!(
            recorded_updates,
            history.iter().map(|(_, ts)| *ts).collect::<Vec<_>>()
        );
        let recorded_fps: Vec<u8> = provider
            .history
            .iter()
            .filter_map(|h| u8::from_str_radix(&h.fingerprint[..2], 16).ok())
            .collect();
        assert_eq!(recorded_fps.len(), history.len());
        assert_eq!(
            recorded_fps,
            history.iter().map(|(fp, _)| *fp).collect::<Vec<_>>()
        );
    }
}

pub fn refresh_peer_cert_store_from_disk() -> bool {
    ensure_peer_cert_store_loaded();
    reload_peer_cert_store_from_disk()
}

pub fn current_peer_fingerprint(peer: &[u8; 32]) -> Option<[u8; 32]> {
    current_peer_fingerprint_for_provider(peer, None)
}

pub fn current_peer_fingerprint_for_provider(
    peer: &[u8; 32],
    provider: Option<&str>,
) -> Option<[u8; 32]> {
    ensure_peer_cert_store_loaded();
    let map = read_lock(&*PEER_CERTS);
    let stores = map.get(peer)?;
    if let Some(id) = provider {
        stores.get(id).map(|store| store.current.fingerprint)
    } else {
        stores
            .values()
            .next()
            .map(|store| store.current.fingerprint)
    }
}

/// Manually verify DNS TXT record for `domain`.
#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DnsVerification<'a> {
    pub domain: &'a str,
    pub verified: bool,
}

pub fn dns_verify(domain: &str) -> DnsVerification<'_> {
    let mut params = foundation_serialization::json::Map::new();
    params.insert(
        "domain".to_string(),
        foundation_serialization::json::Value::String(domain.to_string()),
    );
    let v = crate::gateway::dns::dns_lookup(&foundation_serialization::json::Value::Object(params));
    let verified = v.get("verified").and_then(|b| b.as_bool()).unwrap_or(false);
    DnsVerification { domain, verified }
}

pub fn record_ip_drop(ip: &std::net::SocketAddr) {
    peer::record_ip_drop(ip);
}

/// Broadcast local reputation scores to known peers.
pub fn reputation_sync() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_SYNC: AtomicU64 = AtomicU64::new(0);
    const MIN_INTERVAL: u64 = 5;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = LAST_SYNC.load(Ordering::Relaxed);
    if now.saturating_sub(last) < MIN_INTERVAL {
        return;
    }
    if LAST_SYNC
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    if !crate::compute_market::scheduler::reputation_gossip_enabled() {
        return;
    }
    let peers = peer::known_peers_with_info();
    if peers.is_empty() {
        return;
    }
    let entries = crate::compute_market::scheduler::reputation_snapshot();
    if entries.is_empty() {
        return;
    }
    let sk = load_net_key();
    turbine::broadcast_reputation(&entries, &sk, &peers);
}

/// Return current reputation score for `peer`.
#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReputationScore<'a> {
    pub peer: &'a str,
    pub score: i64,
}

pub fn reputation_show(peer: &str) -> ReputationScore<'_> {
    ReputationScore {
        peer,
        score: crate::compute_market::scheduler::reputation_get(peer),
    }
}

/// Current gossip protocol version.
pub const PROTOCOL_VERSION: u16 = SUPPORTED_VERSION;

/// Feature bits required for peer connections.
pub const COMPUTE_MARKET_V1: u32 = crate::p2p::FeatureBit::ComputeMarketV1 as u32;
pub const REQUIRED_FEATURES: u32 =
    (crate::p2p::FeatureBit::FeeRoutingV2 as u32) | COMPUTE_MARKET_V1;

/// Feature bits this node advertises.
#[cfg(feature = "quic")]
pub const LOCAL_FEATURES: u32 = REQUIRED_FEATURES | (crate::p2p::FeatureBit::QuicTransport as u32);
#[cfg(not(feature = "quic"))]
pub const LOCAL_FEATURES: u32 = REQUIRED_FEATURES;

/// A minimal TCP gossip node.
pub struct Node {
    addr: SocketAddr,
    peers: PeerSet,
    relay: std::sync::Arc<Relay>,
    chain: Arc<Mutex<Blockchain>>,
    key: SigningKey,
    quic_addr: Option<SocketAddr>,
    #[cfg(feature = "quic")]
    quic_advert: Option<transport_quic::CertAdvertisement>,
    #[cfg(not(feature = "quic"))]
    quic_cert: Option<Bytes>,
}

impl Node {
    /// Create a new node bound to `addr` and seeded with `peers`.
    pub fn new(addr: SocketAddr, peers: Vec<SocketAddr>, bc: Blockchain) -> Self {
        Self::new_with_quic(addr, peers, bc, None)
    }

    pub fn new_with_quic(
        addr: SocketAddr,
        peers: Vec<SocketAddr>,
        bc: Blockchain,
        quic: Option<(SocketAddr, Bytes)>,
    ) -> Self {
        let key = load_net_key();
        #[cfg(feature = "quic")]
        let (quic_addr, quic_advert) = match quic {
            Some((addr, cert)) => {
                let fingerprint = transport_quic::fingerprint(cert.as_ref());
                (
                    Some(addr),
                    Some(transport_quic::CertAdvertisement {
                        cert: cert.clone(),
                        fingerprint,
                        previous: Vec::new(),
                        verifying_key: None,
                        issued_at: None,
                    }),
                )
            }
            None => {
                let _ = transport_quic::initialize(&key);
                (None, transport_quic::current_advertisement())
            }
        };
        #[cfg(not(feature = "quic"))]
        let (quic_addr, quic_cert) = match quic {
            Some((addr, cert)) => (Some(addr), Some(cert)),
            None => (None, None),
        };
        if let Err(err) = ban_store::store().guard().purge_expired() {
            diagnostics::tracing::warn!(
                target: "net",
                %err,
                "failed to purge expired bans on startup"
            );
        }
        let relay = std::sync::Arc::new(Relay::default());
        set_gossip_relay(std::sync::Arc::clone(&relay));
        Self {
            addr,
            peers: PeerSet::new(peers),
            chain: Arc::new(Mutex::new(bc)),
            key,
            relay,
            quic_addr,
            #[cfg(feature = "quic")]
            quic_advert,
            #[cfg(not(feature = "quic"))]
            quic_cert,
        }
    }

    /// Start the listener thread handling inbound gossip.
    pub fn start(&self) -> io::Result<thread::JoinHandle<()>> {
        let flag = ShutdownFlag::new();
        self.start_with_flag(&flag)
    }

    /// Start the listener thread handling inbound gossip that stops when `shutdown` is triggered.
    pub fn start_with_flag(&self, shutdown: &ShutdownFlag) -> io::Result<thread::JoinHandle<()>> {
        let listener = bind_tcp_listener(self.addr)?;
        listener.set_nonblocking(true)?;
        let stop = shutdown.as_arc();
        let peers = self.peers.clone();
        let chain = Arc::clone(&self.chain);
        Ok(thread::spawn(move || loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    let addr = Some(addr);
                    let mut buf = Vec::new();
                    if stream.read_to_end(&mut buf).is_ok() {
                        #[cfg(feature = "telemetry")]
                        if crate::telemetry::should_log("p2p") {
                            let trace = crate::telemetry::log_context();
                            let span = crate::log_context!(tx = *blake3::hash(&buf).as_bytes());
                            span.in_scope(|| {
                                diagnostics::tracing::info!(parent: &trace, peer = ?addr, len = buf.len(), "recv_msg");
                            });
                        }
                        if let Ok(msg) = message::decode(&buf) {
                            peers.handle_message(msg, addr, &chain);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }))
    }

    /// Broadcast a transaction to all known peers.
    pub fn broadcast_tx(&self, tx: SignedTransaction) {
        self.broadcast_payload(Payload::Tx(tx));
    }

    /// Broadcast a blob transaction to all known peers.
    pub fn broadcast_blob_tx(&self, tx: BlobTx) {
        self.broadcast_payload(Payload::BlobTx(tx));
    }

    /// Broadcast a blob shard to all known peers.
    pub fn broadcast_blob_chunk(&self, chunk: BlobChunk) {
        self.broadcast_payload(Payload::BlobChunk(chunk));
    }

    /// Broadcast the current chain to all known peers.
    pub fn broadcast_chain(&self) {
        if let Ok(bc) = self.chain.lock() {
            self.broadcast_payload(Payload::Chain(bc.chain.clone()));
        }
    }

    /// Perform peer discovery by handshaking with known peers and exchanging address lists.
    pub fn discover_peers(&self) {
        let peers = self.peers.bootstrap();
        // send handshake to each peer
        let agent = format!("blockd/{}", env!("CARGO_PKG_VERSION"));
        let nonce = OsRng::default().next_u64();
        let transport = if self.quic_addr.is_some() {
            Transport::Quic
        } else {
            Transport::Tcp
        };
        #[cfg(feature = "quic")]
        let (quic_provider, quic_capabilities) = {
            let meta = transport_registry().map(|registry| registry.metadata());
            let provider = meta
                .as_ref()
                .map(|m| m.id.to_string())
                .or_else(|| crate::net::transport_quic::provider_id().map(str::to_string));
            let caps = meta
                .map(|m| {
                    m.capabilities
                        .iter()
                        .map(|cap| capability_label(*cap).to_string())
                        .collect()
                })
                .unwrap_or_default();
            (provider, caps)
        };
        #[cfg(feature = "quic")]
        let (quic_cert, quic_fp, quic_prev) = match &self.quic_advert {
            Some(advert) => (
                Some(advert.cert.clone()),
                Some(Bytes::from(advert.fingerprint.to_vec())),
                advert
                    .previous
                    .iter()
                    .map(|fp| Bytes::from(fp.to_vec()))
                    .collect::<Vec<Bytes>>(),
            ),
            None => (None, None, Vec::new()),
        };
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: LOCAL_FEATURES,
            agent,
            nonce,
            transport,
            quic_addr: self.quic_addr,
            #[cfg(feature = "quic")]
            quic_cert,
            #[cfg(not(feature = "quic"))]
            quic_cert: self.quic_cert.clone(),
            #[cfg(feature = "quic")]
            quic_fingerprint: quic_fp,
            #[cfg(feature = "quic")]
            quic_fingerprint_previous: quic_prev,
            #[cfg(not(feature = "quic"))]
            quic_fingerprint: None,
            #[cfg(not(feature = "quic"))]
            quic_fingerprint_previous: Vec::new(),
            #[cfg(feature = "quic")]
            quic_provider,
            #[cfg(feature = "quic")]
            quic_capabilities,
            #[cfg(not(feature = "quic"))]
            quic_provider: None,
            #[cfg(not(feature = "quic"))]
            quic_capabilities: Vec::new(),
        };
        if let Some(hs_msg) = self.sign_payload(Payload::Handshake(hello), "handshake") {
            for p in &peers {
                let _ = send_msg(*p, &hs_msg);
            }
        }
        // advertise our peer set
        let mut addrs = peers.clone();
        addrs.push(self.addr);
        if let Some(hello_msg) = self.sign_payload(Payload::Hello(addrs), "hello") {
            for p in self.peers.list() {
                let _ = send_msg(p, &hello_msg);
            }
        }
    }

    /// Snapshot known peer addresses.
    pub fn peer_addrs(&self) -> Vec<SocketAddr> {
        self.peers.list()
    }

    /// Add a peer address to this node.
    pub fn add_peer(&self, addr: SocketAddr) {
        self.peers.add(addr);
    }

    /// Remove a peer address from this node.
    pub fn remove_peer(&self, addr: SocketAddr) {
        self.peers.remove(addr);
    }

    /// Clear all peers from this node.
    pub fn clear_peers(&self) {
        self.peers.clear();
    }

    /// Return this node's listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Load seed peer addresses from `config` and perform discovery.
    pub fn discover_peers_from_file<P: AsRef<std::path::Path>>(&self, config: P) {
        if let Ok(data) = fs::read_to_string(config) {
            for line in data.lines() {
                if let Ok(addr) = line.trim().parse::<SocketAddr>() {
                    self.peers.add(addr);
                }
            }
        }
        self.discover_peers();
    }

    /// Access the underlying blockchain.
    pub fn blockchain(&self) -> std::sync::MutexGuard<'_, Blockchain> {
        self.chain.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn broadcast_payload(&self, body: Payload) {
        if let Some(msg) = self.sign_payload(body.clone(), "broadcast") {
            self.broadcast(&msg);
        }
    }

    fn sign_payload(&self, body: Payload, context: &str) -> Option<Message> {
        match Message::new(body, &self.key) {
            Ok(msg) => Some(msg),
            Err(err) => {
                diagnostics::tracing::error!(
                    target = "net",
                    context,
                    reason = %err,
                    "message_encode_failed"
                );
                None
            }
        }
    }

    fn broadcast(&self, msg: &Message) {
        let peers = self.peers.list_with_info();
        if std::env::var("TB_GOSSIP_ALGO").ok().as_deref() == Some("turbine") {
            turbine::broadcast(msg, &peers);
            return;
        }
        if let Payload::Block(shard, _) = &msg.body {
            let mut map: HashMap<OverlayPeerId, (SocketAddr, Transport, Option<Bytes>)> =
                HashMap::new();
            for (addr, t, c) in peers {
                if let Some(pk) = pk_from_addr(&addr) {
                    if let Ok(peer) = overlay_peer_from_bytes(&pk) {
                        map.insert(peer, (addr, t, c));
                    }
                }
            }
            self.relay.broadcast_shard(*shard as ShardId, msg, &map);
        } else {
            self.relay.broadcast(msg, &peers);
        }
    }
}

fn bind_tcp_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    listener::bind_sync("net", "gossip_listener_bind_failed", addr)
}

pub(crate) fn send_msg(addr: SocketAddr, msg: &Message) -> std::io::Result<()> {
    let mut rng = rand::thread_rng();
    if let Ok(loss_str) = std::env::var("TB_NET_PACKET_LOSS") {
        if let Ok(loss) = loss_str.parse::<f64>() {
            if rng.gen_bool(loss) {
                return Ok(());
            }
        }
    }
    if let Ok(jitter_str) = std::env::var("TB_NET_JITTER_MS") {
        if let Ok(jitter) = jitter_str.parse::<u64>() {
            let delay = rng.gen_range(0..=jitter);
            std::thread::sleep(Duration::from_millis(delay));
        }
    }
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
    let bytes = message::encode_message(msg)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("serialize: {err}")))?;
    #[cfg(feature = "telemetry")]
    if crate::telemetry::should_log("p2p") {
        let span = crate::log_context!(tx = *blake3::hash(&bytes).as_bytes());
        diagnostics::tracing::info!(parent: &span, peer = %addr, len = bytes.len(), "send_msg");
    }
    stream.write_all(&bytes)?;
    crate::net::peer::record_send(addr, bytes.len());
    Ok(())
}

#[cfg(feature = "quic")]
pub(crate) fn send_quic_msg(
    addr: SocketAddr,
    cert: &Bytes,
    msg: &Message,
) -> Result<(), quic::ConnectError> {
    use crate::net::quic;
    #[cfg(feature = "telemetry")]
    use crate::telemetry::QUIC_FALLBACK_TCP_TOTAL;
    let bytes = message::encode_message(msg)
        .map_err(|err| quic::ConnectError::Other(anyhow!("serialize: {err}")))?;
    let cert = quic::certificate_from_der(cert.clone()).map_err(quic::ConnectError::Other)?;
    let res = runtime::block_on(async {
        let conn = quic::get_connection(addr, &cert).await?;
        if let Err(e) = quic::send(&conn, &bytes).await {
            quic::drop_connection(&addr);
            return Err(quic::ConnectError::Other(anyhow!(e)));
        }
        Ok(())
    });
    match res {
        Err(quic::ConnectError::Handshake(_)) => {
            #[cfg(feature = "telemetry")]
            QUIC_FALLBACK_TCP_TOTAL.inc();
            send_msg(addr, msg).map_err(|e| quic::ConnectError::Other(anyhow!(e)))?;
            Ok(())
        }
        other => other,
    }
}

#[cfg(not(feature = "quic"))]
pub(crate) fn send_quic_msg(
    _addr: SocketAddr,
    _cert: &Bytes,
    _msg: &Message,
) -> Result<(), quic::ConnectError> {
    Err(quic::ConnectError::Other(anyhow!(
        "quic feature not enabled"
    )))
}

pub fn load_net_key() -> SigningKey {
    let path = std::env::var("TB_NET_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("net_key")
        });
    if let Ok(bytes) = fs::read(&path) {
        if bytes.len() == 64 {
            let mut arr = [0u8; 64];
            arr.copy_from_slice(&bytes);
            if let Ok(sk) = SigningKey::from_keypair_bytes(&arr) {
                return sk;
            }
        }
    }
    let mut seed = [0u8; 32];
    if let Ok(s) = std::env::var("TB_NET_KEY_SEED") {
        let hash = blake3::hash(s.as_bytes());
        seed.copy_from_slice(hash.as_bytes());
    } else {
        let mut rng = OsRng::default();
        rng.fill_bytes(&mut seed);
    }
    let sk = SigningKey::from_bytes(&seed);
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            diagnostics::tracing::warn!(
                path = %path.display(),
                reason = %err,
                "net_key_dir_create_failed"
            );
        }
    }
    if let Err(err) = fs::write(&path, sk.to_keypair_bytes()) {
        diagnostics::tracing::error!(
            path = %path.display(),
            reason = %err,
            "net_key_persist_failed"
        );
    }
    sk
}
