pub mod a_star;
pub mod ban_store;
pub mod discovery;
mod message;
pub mod peer;
pub mod peer_metrics_store;
#[cfg(feature = "quic")]
pub mod quic;
#[cfg(feature = "quic")]
pub mod quic_stats;
#[cfg(feature = "quic")]
pub mod transport_quic;
pub mod uptime;
#[cfg(not(feature = "quic"))]
pub mod quic {
    use super::peer::HandshakeError;
    use anyhow::Error;

    #[derive(Debug)]
    pub enum ConnectError {
        Handshake(HandshakeError),
        Other(Error),
    }
}
pub mod partition_watch;
pub mod turbine;

use crate::net::peer::pk_from_addr;
use crate::{
    gossip::relay::{Relay, RelayStatus},
    BlobTx, Blockchain, ShutdownFlag, SignedTransaction,
};
use anyhow::anyhow;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use blake3;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::SigningKey;
use hex;
use ledger::address::ShardId;
use once_cell::sync::{Lazy, OnceCell};
use rand::Rng;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Entry, HashMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc::channel, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[cfg(all(feature = "quic", feature = "telemetry"))]
use crate::telemetry::{
    sampled_observe, QUIC_BYTES_RECV_TOTAL, QUIC_BYTES_SENT_TOTAL, QUIC_CERT_ROTATION_TOTAL,
    QUIC_CONN_LATENCY_SECONDS, QUIC_DISCONNECT_TOTAL, QUIC_ENDPOINT_REUSE_TOTAL,
    QUIC_HANDSHAKE_FAIL_TOTAL, QUIC_PROVIDER_CONNECT_TOTAL, QUIC_RETRANSMIT_TOTAL,
};
#[cfg(feature = "quic")]
use transport::{
    self, Config as TransportConfig, DefaultFactory as TransportDefaultFactory,
    ProviderRegistry as TransportProviderRegistry, QuinnCallbacks, QuinnDisconnect, S2nCallbacks,
    TransportCallbacks, TransportFactory,
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

#[cfg(feature = "quic")]
pub fn quic_stats() -> Vec<QuicStatsEntry> {
    quic_stats::snapshot()
}

#[cfg(not(feature = "quic"))]
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
    *TRANSPORT_FACTORY.write().unwrap() = factory;
}

#[cfg(feature = "quic")]
pub fn configure_transport(cfg: &TransportConfig) -> anyhow::Result<()> {
    let callbacks = build_transport_callbacks();
    let factory = TRANSPORT_FACTORY.read().unwrap().clone();
    let registry = factory.create(cfg, &callbacks)?;
    *TRANSPORT_REGISTRY.write().unwrap() = Some(registry);
    Ok(())
}

#[cfg(feature = "quic")]
pub(crate) fn transport_registry() -> Option<TransportProviderRegistry> {
    {
        let guard = TRANSPORT_REGISTRY.read().unwrap();
        if guard.is_some() {
            return guard.clone();
        }
    }
    if let Err(err) = configure_transport(&TransportConfig::default()) {
        #[cfg(feature = "telemetry")]
        tracing::warn!(reason = %err, "transport_configure_default_failed");
        #[cfg(not(feature = "telemetry"))]
        eprintln!("transport_configure_default_failed: {err}");
    }
    TRANSPORT_REGISTRY.read().unwrap().clone()
}

#[cfg(feature = "quic")]
fn build_transport_callbacks() -> TransportCallbacks {
    let mut callbacks = TransportCallbacks::default();

    #[cfg(feature = "quic")]
    let provider_counter: Arc<dyn Fn(&'static str) + Send + Sync + 'static> = {
        let cb = Arc::new(|provider: &'static str| {
            #[cfg(feature = "telemetry")]
            QUIC_PROVIDER_CONNECT_TOTAL
                .with_label_values(&[provider])
                .inc();
        });
        cb
    };

    {
        let quinn = &mut callbacks.quinn;
        #[cfg(feature = "quinn")]
        {
            quinn.provider_connect = Some(provider_counter.clone());
        }
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
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[peer_label.as_str(), mapped.as_str()])
                        .inc();
                }
                tracing::error!(reason = mapped.as_str(), "quic_connect_fail");
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
        }));

        quinn.bytes_received = Some(Arc::new(|_addr: SocketAddr, bytes: u64| {
            #[cfg(feature = "telemetry")]
            QUIC_BYTES_RECV_TOTAL.inc_by(bytes);
        }));

        quinn.disconnect = Some(Arc::new(|_addr: SocketAddr, reason: QuinnDisconnect| {
            #[cfg(feature = "telemetry")]
            {
                let label = reason.label();
                QUIC_DISCONNECT_TOTAL
                    .with_label_values(&[label.as_ref()])
                    .inc();
            }
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
            QUIC_CERT_ROTATION_TOTAL.with_label_values(&[label]).inc();
        }));

        s2n.handshake_failure = Some(Arc::new(|reason: &str| {
            #[cfg(feature = "telemetry")]
            {
                let peer_label = quic_stats::peer_label(None);
                QUIC_HANDSHAKE_FAIL_TOTAL
                    .with_label_values(&[peer_label.as_str(), reason])
                    .inc();
            }
        }));

        s2n.retransmit = Some(Arc::new(|count: u64| {
            #[cfg(feature = "telemetry")]
            QUIC_RETRANSMIT_TOTAL.inc_by(count);
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
    cert: Vec<u8>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    cert: Option<String>,
    updated_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct PeerCertDiskEntry {
    peer: String,
    #[serde(default)]
    providers: Vec<ProviderDiskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<CertDiskRecord>,
    history: Vec<CertDiskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rotations: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ProviderDiskRecord {
    provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<CertDiskRecord>,
    #[serde(default)]
    history: Vec<CertDiskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    *GOSSIP_RELAY.write().unwrap() = Some(relay);
}

pub fn gossip_status() -> Option<RelayStatus> {
    GOSSIP_RELAY
        .read()
        .unwrap()
        .as_ref()
        .map(|relay| relay.status())
}

pub fn register_shard_peer(shard: ShardId, peer: [u8; 32]) {
    if let Some(relay) = GOSSIP_RELAY.read().unwrap().as_ref() {
        relay.register_peer(shard, peer);
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct QuicStatsEntry {
    pub peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    dirs::home_dir()
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
                if let Ok(bytes) = hex::decode(key_hex.trim()) {
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
                if let Ok(bytes) = hex::decode(node_hex.trim()) {
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
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        if let Ok(mut ciphertext) = cipher.encrypt(Nonce::from_slice(&nonce), cert) {
            let mut payload = Vec::with_capacity(nonce.len() + ciphertext.len());
            payload.extend_from_slice(&nonce);
            payload.append(&mut ciphertext);
            return Some(format!("{}{}", CERT_BLOB_PREFIX, B64.encode(payload)));
        }
    }
    Some(B64.encode(cert))
}

fn decrypt_cert_blob(data: &str) -> Option<Vec<u8>> {
    if data.starts_with(CERT_BLOB_PREFIX) {
        let payload = B64.decode(data[CERT_BLOB_PREFIX.len()..].as_bytes()).ok()?;
        if payload.len() < 12 {
            return None;
        }
        let (nonce, ciphertext) = payload.split_at(12);
        let key = peer_cert_encryption_key()?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()
    } else {
        B64.decode(data.as_bytes()).ok()
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
        Ok(data) => match serde_json::from_slice::<Vec<PeerCertDiskEntry>>(&data) {
            Ok(entries) => {
                let mut rebuilt = HashMap::new();
                let legacy_provider = if cfg!(feature = "s2n-quic") {
                    transport::ProviderKind::S2nQuic.id().to_string()
                } else {
                    transport::ProviderKind::Quinn.id().to_string()
                };
                for entry in entries {
                    if let Ok(bytes) = hex::decode(&entry.peer) {
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
                let mut map = PEER_CERTS.write().unwrap();
                *map = rebuilt;
                true
            }
            Err(_) => false,
        },
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                let mut map = PEER_CERTS.write().unwrap();
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
    let mut guard = PEER_CERT_WATCH_PATH.lock().unwrap();
    if guard.as_ref() == Some(&path) {
        return;
    }
    *guard = Some(path.clone());
    thread::spawn(move || {
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        if watcher.watch(&parent, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        for res in rx {
            if let Ok(event) = res {
                if !matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    continue;
                }
                let mut relevant = false;
                for changed in &event.paths {
                    if changed == &path {
                        relevant = true;
                        break;
                    }
                    if let (Some(file), Some(target)) = (changed.file_name(), path.file_name()) {
                        if file == target {
                            relevant = true;
                            break;
                        }
                    }
                }
                if relevant {
                    let _ = reload_peer_cert_store_from_disk();
                }
            }
        }
    });
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
    let entries: Vec<PeerCertDiskEntry> = map
        .iter()
        .map(|(peer, providers)| {
            let provider_records: Vec<ProviderDiskRecord> = providers
                .iter()
                .map(|(provider, store)| ProviderDiskRecord {
                    provider: provider.clone(),
                    current: Some(snapshot_to_disk(&store.current)),
                    history: store.history.iter().map(snapshot_to_disk).collect(),
                    rotations: Some(store.rotations),
                })
                .collect();
            PeerCertDiskEntry {
                peer: hex::encode(peer),
                providers: provider_records,
                current: None,
                history: Vec::new(),
                rotations: None,
            }
        })
        .collect();
    if let Ok(json) = serde_json::to_vec_pretty(&entries) {
        let _ = fs::write(path, json);
    }
}

fn disk_to_snapshot(record: &CertDiskRecord) -> Option<CertSnapshot> {
    let fingerprint_bytes = hex::decode(&record.fingerprint).ok()?;
    if fingerprint_bytes.len() != 32 {
        return None;
    }
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&fingerprint_bytes);
    let cert = record
        .cert
        .as_ref()
        .and_then(|c| decrypt_cert_blob(c))
        .unwrap_or_default();
    if !cert.is_empty() {
        let hash = blake3::hash(&cert);
        if hash.as_bytes() != &fingerprint {
            return None;
        }
    }
    Some(CertSnapshot {
        fingerprint,
        cert,
        updated_at: record.updated_at,
    })
}

fn snapshot_to_disk(snapshot: &CertSnapshot) -> CertDiskRecord {
    CertDiskRecord {
        fingerprint: hex::encode(snapshot.fingerprint),
        cert: encrypt_cert_blob(&snapshot.cert),
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
            cert: Vec::new(),
            updated_at: now,
        });
    }
}

pub fn record_peer_certificate(
    peer: &[u8; 32],
    provider: &str,
    cert: Vec<u8>,
    fingerprint: [u8; 32],
    previous: Vec<[u8; 32]>,
) {
    if !cert.is_empty() {
        let computed = blake3::hash(&cert);
        if computed.as_bytes() != &fingerprint {
            return;
        }
    }
    ensure_peer_cert_store_loaded();
    let mut map = PEER_CERTS.write().unwrap();
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
                    let label = hex::encode(peer);
                    crate::telemetry::QUIC_CERT_ROTATION_TOTAL
                        .with_label_values(&[label.as_str()])
                        .inc();
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
        let map = PEER_CERTS.read().unwrap();
        let result = verify_peer_fingerprint_inner(&map, peer, None, fingerprint);
        if result || fingerprint.is_none() {
            return result;
        }
    }
    if reload_peer_cert_store_from_disk() {
        let map = PEER_CERTS.read().unwrap();
        return verify_peer_fingerprint_inner(&map, peer, None, fingerprint);
    }
    false
}

pub fn peer_cert_snapshot() -> Vec<PeerCertSnapshot> {
    ensure_peer_cert_store_loaded();
    let map = PEER_CERTS.read().unwrap();
    let mut snapshots = Vec::new();
    for (peer, stores) in map.iter() {
        for (provider, store) in stores.iter() {
            snapshots.push(PeerCertSnapshot {
                peer: *peer,
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
        fingerprint: hex::encode(snapshot.fingerprint),
        updated_at: snapshot.updated_at,
        age_secs: now.saturating_sub(snapshot.updated_at),
        has_certificate: !snapshot.cert.is_empty(),
    }
}

pub fn peer_cert_history() -> Vec<PeerCertHistoryEntry> {
    ensure_peer_cert_store_loaded();
    let now = unix_now();
    let map = PEER_CERTS.read().unwrap();
    let mut entries: Vec<_> = Vec::new();
    for (peer, stores) in map.iter() {
        for (provider, store) in stores.iter() {
            entries.push(PeerCertHistoryEntry {
                peer: hex::encode(peer),
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
    entries.sort_by(|a, b| {
        let ord = a.peer.cmp(&b.peer);
        if ord == std::cmp::Ordering::Equal {
            a.provider.cmp(&b.provider)
        } else {
            ord
        }
    });
    entries
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
    let map = PEER_CERTS.read().unwrap();
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
pub fn dns_verify(domain: &str) -> serde_json::Value {
    let v = crate::gateway::dns::dns_lookup(&serde_json::json!({ "domain": domain }));
    let verified = v.get("verified").and_then(|b| b.as_bool()).unwrap_or(false);
    serde_json::json!({ "domain": domain, "verified": verified })
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
pub fn reputation_show(peer: &str) -> serde_json::Value {
    serde_json::json!({
        "peer": peer,
        "score": crate::compute_market::scheduler::reputation_get(peer),
    })
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
    quic_cert: Option<Vec<u8>>,
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
        quic: Option<(SocketAddr, Vec<u8>)>,
    ) -> Self {
        let key = load_net_key();
        #[cfg(feature = "quic")]
        let (quic_addr, quic_advert) = match quic {
            Some((addr, cert)) => {
                let fingerprint = transport_quic::fingerprint(&cert);
                (
                    Some(addr),
                    Some(transport_quic::CertAdvertisement {
                        cert,
                        fingerprint,
                        previous: Vec::new(),
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
        ban_store::store()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .purge_expired();
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
    pub fn start(&self) -> thread::JoinHandle<()> {
        let flag = ShutdownFlag::new();
        self.start_with_flag(&flag)
    }

    /// Start the listener thread handling inbound gossip that stops when `shutdown` is triggered.
    pub fn start_with_flag(&self, shutdown: &ShutdownFlag) -> thread::JoinHandle<()> {
        let listener = TcpListener::bind(self.addr).unwrap_or_else(|e| panic!("bind: {e}"));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|e| panic!("nonblock: {e}"));
        let stop = shutdown.as_arc();
        let peers = self.peers.clone();
        let chain = Arc::clone(&self.chain);
        thread::spawn(move || loop {
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
                                tracing::info!(parent: &trace, peer = ?addr, len = buf.len(), "recv_msg");
                            });
                        }
                        if let Ok(msg) = bincode::deserialize::<Message>(&buf) {
                            peers.handle_message(msg, addr, &chain);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        })
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
        let nonce = OsRng.next_u64();
        let transport = if self.quic_addr.is_some() {
            Transport::Quic
        } else {
            Transport::Tcp
        };
        #[cfg(feature = "quic")]
        let (quic_provider, quic_capabilities) = {
            let meta = transport_registry().map(|registry| registry.metadata());
            let provider = meta.as_ref().map(|m| m.id.to_string());
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
                Some(advert.fingerprint.to_vec()),
                advert
                    .previous
                    .iter()
                    .map(|fp| fp.to_vec())
                    .collect::<Vec<Vec<u8>>>(),
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
        let hs_msg = Message::new(Payload::Handshake(hello), &self.key);
        for p in &peers {
            let _ = send_msg(*p, &hs_msg);
        }
        // advertise our peer set
        let mut addrs = peers.clone();
        addrs.push(self.addr);
        let hello_msg = Message::new(Payload::Hello(addrs), &self.key);
        for p in self.peers.list() {
            let _ = send_msg(p, &hello_msg);
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
        let msg = Message::new(body.clone(), &self.key);
        self.broadcast(&msg);
    }

    fn broadcast(&self, msg: &Message) {
        let peers = self.peers.list_with_info();
        if std::env::var("TB_GOSSIP_ALGO").ok().as_deref() == Some("turbine") {
            turbine::broadcast(msg, &peers);
            return;
        }
        if let Payload::Block(shard, _) = &msg.body {
            let mut map: HashMap<[u8; 32], (SocketAddr, Transport, Option<Vec<u8>>)> =
                HashMap::new();
            for (addr, t, c) in peers {
                if let Some(pk) = pk_from_addr(&addr) {
                    map.insert(pk, (addr, t, c));
                }
            }
            self.relay.broadcast_shard(*shard as ShardId, msg, &map);
        } else {
            self.relay.broadcast(msg, &peers);
        }
    }
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
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    #[cfg(feature = "telemetry")]
    if crate::telemetry::should_log("p2p") {
        let span = crate::log_context!(tx = *blake3::hash(&bytes).as_bytes());
        tracing::info!(parent: &span, peer = %addr, len = bytes.len(), "send_msg");
    }
    stream.write_all(&bytes)?;
    crate::net::peer::record_send(addr, bytes.len());
    Ok(())
}

#[cfg(feature = "quic")]
pub(crate) fn send_quic_msg(
    addr: SocketAddr,
    cert: &[u8],
    msg: &Message,
) -> Result<(), quic::ConnectError> {
    use crate::net::quic;
    #[cfg(feature = "telemetry")]
    use crate::telemetry::QUIC_FALLBACK_TCP_TOTAL;
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    let cert = quic::certificate_from_der(cert.to_vec()).map_err(quic::ConnectError::Other)?;
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
    _cert: &[u8],
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
            dirs::home_dir()
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
        let mut rng = OsRng;
        rng.fill_bytes(&mut seed);
    }
    let sk = SigningKey::from_bytes(&seed);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, sk.to_keypair_bytes()).unwrap_or_else(|e| panic!("write net_key: {e}"));
    sk
}
