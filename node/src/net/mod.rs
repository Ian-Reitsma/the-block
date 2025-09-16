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
use crate::{gossip::relay::Relay, BlobTx, Blockchain, ShutdownFlag, SignedTransaction};
use anyhow::anyhow;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use blake3;
use ed25519_dalek::SigningKey;
use hex;
use ledger::address::ShardId;
use once_cell::sync::{Lazy, OnceCell};
use rand::Rng;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{atomic::Ordering, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

pub use crate::p2p::handshake::{Hello, Transport, SUPPORTED_VERSION};
pub use message::{BlobChunk, Message, Payload};
pub use peer::ReputationUpdate;
pub use peer::{
    clear_peer_metrics, clear_throttle, export_all_peer_stats, export_peer_stats, known_peers,
    load_peer_metrics, p2p_max_bytes_per_sec, p2p_max_per_sec, peer_reputation_decay, peer_stats,
    peer_stats_all, peer_stats_map, persist_peer_metrics, recent_handshake_failures,
    record_request, reset_peer_metrics, rotate_peer_key, set_max_peer_metrics,
    set_metrics_aggregator, set_metrics_export_dir, set_p2p_max_bytes_per_sec, set_p2p_max_per_sec,
    set_peer_metrics_compress, set_peer_metrics_export, set_peer_metrics_export_quota,
    set_peer_metrics_path, set_peer_metrics_retention, set_peer_metrics_sample_rate,
    set_peer_reputation_decay, set_track_drop_reasons, set_track_handshake_fail, throttle_peer,
    DropReason, HandshakeError, PeerMetrics, PeerReputation, PeerSet, PeerStat,
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

const PEER_CERT_STORE_FILE: &str = "quic_peer_certs.json";
const MAX_PEER_CERT_HISTORY: usize = 4;

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
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<CertDiskRecord>,
    history: Vec<CertDiskRecord>,
}

static PEER_CERTS: Lazy<RwLock<HashMap<[u8; 32], PeerCertStore>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static PEER_CERTS_LOADED: OnceCell<()> = OnceCell::new();

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct QuicStatsEntry {
    pub peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub retransmits: u64,
    pub endpoint_reuse: u64,
    pub handshake_failures: u64,
    pub last_updated: u64,
}

#[derive(Clone, Serialize)]
pub struct PeerCertSnapshot {
    pub peer: [u8; 32],
    pub fingerprint: [u8; 32],
    pub updated_at: u64,
}

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

fn ensure_peer_cert_store_loaded() {
    if PEER_CERTS_LOADED.get().is_some() {
        return;
    }
    let mut map = PEER_CERTS.write().unwrap();
    if let Ok(data) = fs::read(peer_cert_store_path()) {
        if let Ok(entries) = serde_json::from_slice::<Vec<PeerCertDiskEntry>>(&data) {
            for entry in entries {
                if let Ok(bytes) = hex::decode(&entry.peer) {
                    if bytes.len() != 32 {
                        continue;
                    }
                    let mut peer = [0u8; 32];
                    peer.copy_from_slice(&bytes);
                    if let Some(current) =
                        entry.current.as_ref().and_then(|rec| disk_to_snapshot(rec))
                    {
                        let mut history = VecDeque::new();
                        for rec in &entry.history {
                            if let Some(snapshot) = disk_to_snapshot(rec) {
                                history.push_back(snapshot);
                            }
                        }
                        map.insert(peer, PeerCertStore { current, history });
                    }
                }
            }
        }
    }
    PEER_CERTS_LOADED.set(()).ok();
}

fn persist_peer_cert_store(map: &HashMap<[u8; 32], PeerCertStore>) {
    let path = peer_cert_store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entries: Vec<PeerCertDiskEntry> = map
        .iter()
        .map(|(peer, store)| PeerCertDiskEntry {
            peer: hex::encode(peer),
            current: Some(snapshot_to_disk(&store.current)),
            history: store.history.iter().map(snapshot_to_disk).collect(),
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
        .and_then(|c| B64.decode(c.as_bytes()).ok())
        .unwrap_or_default();
    Some(CertSnapshot {
        fingerprint,
        cert,
        updated_at: record.updated_at,
    })
}

fn snapshot_to_disk(snapshot: &CertSnapshot) -> CertDiskRecord {
    CertDiskRecord {
        fingerprint: hex::encode(snapshot.fingerprint),
        cert: if snapshot.cert.is_empty() {
            None
        } else {
            Some(B64.encode(&snapshot.cert))
        },
        updated_at: snapshot.updated_at,
    }
}

pub fn record_peer_certificate(
    peer: &[u8; 32],
    cert: Vec<u8>,
    fingerprint: [u8; 32],
    previous: Vec<[u8; 32]>,
) {
    ensure_peer_cert_store_loaded();
    let mut map = PEER_CERTS.write().unwrap();
    let now = unix_now();
    let entry = map.entry(*peer).or_insert_with(|| PeerCertStore {
        current: CertSnapshot {
            fingerprint,
            cert: cert.clone(),
            updated_at: now,
        },
        history: VecDeque::new(),
    });
    if entry.current.fingerprint != fingerprint {
        let prev = CertSnapshot {
            fingerprint: entry.current.fingerprint,
            cert: std::mem::take(&mut entry.current.cert),
            updated_at: entry.current.updated_at,
        };
        entry.history.push_front(prev);
        entry.current = CertSnapshot {
            fingerprint,
            cert: cert.clone(),
            updated_at: now,
        };
    } else {
        entry.current.cert = cert.clone();
        entry.current.updated_at = now;
    }
    for fp in previous {
        if entry.current.fingerprint == fp || entry.history.iter().any(|h| h.fingerprint == fp) {
            continue;
        }
        entry.history.push_back(CertSnapshot {
            fingerprint: fp,
            cert: Vec::new(),
            updated_at: now,
        });
    }
    while entry.history.len() > MAX_PEER_CERT_HISTORY {
        entry.history.pop_back();
    }
    persist_peer_cert_store(&map);
}

pub fn verify_peer_fingerprint(peer: &[u8; 32], fingerprint: Option<&[u8; 32]>) -> bool {
    ensure_peer_cert_store_loaded();
    let map = PEER_CERTS.read().unwrap();
    match map.get(peer) {
        Some(store) => {
            if let Some(fp) = fingerprint {
                if fp == &store.current.fingerprint {
                    true
                } else {
                    store.history.iter().any(|h| &h.fingerprint == fp)
                }
            } else {
                false
            }
        }
        None => fingerprint.is_none(),
    }
}

pub fn peer_cert_snapshot() -> Vec<PeerCertSnapshot> {
    ensure_peer_cert_store_loaded();
    let map = PEER_CERTS.read().unwrap();
    map.iter()
        .map(|(peer, store)| PeerCertSnapshot {
            peer: *peer,
            fingerprint: store.current.fingerprint,
            updated_at: store.current.updated_at,
        })
        .collect()
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
    use rustls::Certificate;
    use tokio::runtime::Runtime;
    let bytes = bincode::serialize(msg).unwrap_or_else(|e| panic!("serialize: {e}"));
    let cert = Certificate(cert.to_vec());
    let rt = Runtime::new().unwrap();
    let res = rt.block_on(async {
        let conn = quic::get_connection(addr, cert).await?;
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
