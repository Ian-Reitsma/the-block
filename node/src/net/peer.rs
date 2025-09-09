use super::{load_net_key, send_msg, PROTOCOL_VERSION};
#[cfg(feature = "telemetry")]
use crate::consensus::observer;
use crate::net::message::{Message, Payload};
use crate::p2p::handshake::Transport;
use crate::simple_db::SimpleDb;
use crate::Blockchain;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hex;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use tar::Builder;
use tempfile::NamedTempFile;

use super::ban_store;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Thread-safe peer set used by the gossip layer.
#[derive(Clone, Default)]
pub struct PeerSet {
    addrs: Arc<Mutex<HashSet<SocketAddr>>>,
    authorized: Arc<Mutex<HashSet<[u8; 32]>>>,
    states: Arc<Mutex<HashMap<[u8; 32], PeerState>>>,
    transports: Arc<Mutex<HashMap<SocketAddr, Transport>>>,
    quic: Arc<Mutex<HashMap<SocketAddr, QuicEndpoint>>>,
}

impl PeerSet {
    /// Create a new set seeded with `initial` peers and any persisted peers.
    pub fn new(initial: Vec<SocketAddr>) -> Self {
        let mut set: HashSet<_> = initial.into_iter().collect();
        if let Ok(data) = fs::read_to_string(peer_db_path()) {
            for line in data.lines() {
                if let Ok(addr) = line.trim().parse::<SocketAddr>() {
                    set.insert(addr);
                }
            }
        }
        persist_peers(&set);
        let quic_map = load_quic_peers();
        Self {
            addrs: Arc::new(Mutex::new(set)),
            authorized: Arc::new(Mutex::new(HashSet::new())),
            states: Arc::new(Mutex::new(HashMap::new())),
            transports: Arc::new(Mutex::new(HashMap::new())),
            quic: Arc::new(Mutex::new(quic_map)),
        }
    }

    /// Add a peer to the set.
    pub fn add(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.insert(addr);
            persist_peers(&guard);
        }
        if let Ok(mut map) = self.transports.lock() {
            map.entry(addr).or_insert(Transport::Tcp);
        }
        if let Ok(q) = self.quic.lock() {
            if !q.contains_key(&addr) {
                persist_quic_peers(&q);
            }
        }
    }

    /// Remove a peer from the set.
    pub fn remove(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.remove(&addr);
            persist_peers(&guard);
        }
        if let Ok(mut map) = self.transports.lock() {
            map.remove(&addr);
        }
    }

    /// Clear all peers from the set.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.clear();
            persist_peers(&guard);
        }
        if let Ok(mut map) = self.transports.lock() {
            map.clear();
        }
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.addrs
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Snapshot peers with their advertised transport.
    pub fn list_with_transport(&self) -> Vec<(SocketAddr, Transport)> {
        self.list_with_info()
            .into_iter()
            .map(|(a, t, _)| (a, t))
            .collect()
    }

    /// Snapshot peers with transport and optional QUIC certificate.
    pub fn list_with_info(&self) -> Vec<(SocketAddr, Transport, Option<Vec<u8>>)> {
        let addrs = self.addrs.lock().unwrap_or_else(|e| e.into_inner());
        let transports = self.transports.lock().unwrap_or_else(|e| e.into_inner());
        let quic = self.quic.lock().unwrap_or_else(|e| e.into_inner());
        addrs
            .iter()
            .map(|a| {
                if let Some(info) = quic.get(a) {
                    (info.addr, Transport::Quic, Some(info.cert.clone()))
                } else {
                    (*a, *transports.get(a).unwrap_or(&Transport::Tcp), None)
                }
            })
            .collect()
    }

    /// Record the mapping from address to peer id and allocate metrics entry.
    fn map_addr(&self, addr: SocketAddr, pk: [u8; 32]) {
        if let Ok(mut m) = ADDR_MAP.lock() {
            m.insert(addr, pk);
        }
        let mut metrics = PEER_METRICS.lock().unwrap();
        if let Some(val) = metrics.swap_remove(&pk) {
            metrics.insert(pk, val);
            update_active_gauge(metrics.len());
            return;
        }
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if metrics.len() == max {
            if let Some((old, _)) = metrics.swap_remove_index(0) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&old);
                    if crate::telemetry::should_log("p2p") {
                        let id = hex::encode(old);
                        tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        entry.last_updated = now_secs();
        metrics.insert(pk, entry);
        update_active_gauge(metrics.len());
    }

    /// Record the preferred transport for `addr`.
    pub fn set_transport(&self, addr: SocketAddr, transport: Transport) {
        if let Ok(mut map) = self.transports.lock() {
            map.insert(addr, transport);
        }
    }

    /// Record QUIC endpoint info for `addr`.
    pub fn set_quic(&self, addr: SocketAddr, quic_addr: SocketAddr, cert: Vec<u8>) {
        if let Ok(mut map) = self.quic.lock() {
            map.insert(
                addr,
                QuicEndpoint {
                    addr: quic_addr,
                    cert,
                },
            );
            persist_quic_peers(&map);
        }
    }

    /// Return a randomized list of peers for bootstrapping.
    pub fn bootstrap(&self) -> Vec<SocketAddr> {
        let mut peers = self.list();
        let seed = std::env::var("TB_PEER_SEED")
            .ok()
            .and_then(|v| v.parse().ok());
        let mut rng: StdRng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => {
                StdRng::from_rng(rand::thread_rng()).unwrap_or_else(|_| StdRng::seed_from_u64(0))
            }
        };
        peers.shuffle(&mut rng);
        peers
    }

    fn authorize(&self, pk: [u8; 32]) {
        if let Ok(mut set) = self.authorized.lock() {
            set.insert(pk);
        }
    }

    fn is_authorized(&self, pk: &[u8; 32]) -> bool {
        self.authorized
            .lock()
            .map(|s| s.contains(pk))
            .unwrap_or(false)
    }

    fn check_rate(&self, pk: &[u8; 32]) -> Result<(), PeerErrorCode> {
        if ban_store::store()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_banned(pk)
        {
            return Err(PeerErrorCode::Banned);
        }
        let mut map = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(*pk).or_insert(PeerState {
            count: 0,
            last: Instant::now(),
            banned_until: None,
            shard_tokens: *P2P_SHARD_BURST as f64,
            shard_last: Instant::now(),
        });
        if let Some(until) = entry.banned_until {
            if until > Instant::now() {
                return Err(PeerErrorCode::Banned);
            } else {
                entry.banned_until = None;
                entry.count = 0;
            }
        }
        if entry.last.elapsed() >= Duration::from_secs(1) {
            entry.last = Instant::now();
            entry.count = 0;
        }
        entry.count += 1;
        let allowed = {
            let mut metrics = PEER_METRICS.lock().unwrap();
            let pm = metrics.entry(*pk).or_insert_with(PeerMetrics::default);
            pm.reputation.decay(peer_reputation_decay());
            pm.last_updated = now_secs();
            let score = pm.reputation.score;
            update_reputation_metric(pk, score);
            (p2p_max_per_sec() as f64 * score) as u32
        };
        if entry.count > allowed {
            {
                let mut metrics = PEER_METRICS.lock().unwrap();
                if let Some(pm) = metrics.get_mut(pk) {
                    pm.reputation.penalize(0.9);
                    update_reputation_metric(pk, pm.reputation.score);
                }
            }
            let until = Instant::now() + Duration::from_secs(*P2P_BAN_SECS);
            entry.banned_until = Some(until);
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|e| panic!("time error: {e}"))
                .as_secs()
                + *P2P_BAN_SECS as u64;
            ban_store::store()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .ban(pk, ts);
            return Err(PeerErrorCode::RateLimit);
        }
        Ok(())
    }

    fn check_shard_rate(&self, pk: &[u8; 32], size: usize) -> Result<(), PeerErrorCode> {
        let mut map = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(*pk).or_insert(PeerState {
            count: 0,
            last: Instant::now(),
            banned_until: None,
            shard_tokens: *P2P_SHARD_BURST as f64,
            shard_last: Instant::now(),
        });
        let score = {
            let mut metrics = PEER_METRICS.lock().unwrap();
            let pm = metrics.entry(*pk).or_insert_with(PeerMetrics::default);
            pm.reputation.decay(peer_reputation_decay());
            pm.last_updated = now_secs();
            let s = pm.reputation.score;
            update_reputation_metric(pk, s);
            s
        };
        let now = Instant::now();
        let elapsed = now.duration_since(entry.shard_last).as_secs_f64();
        let rate = *P2P_SHARD_RATE * score;
        let burst = *P2P_SHARD_BURST as f64 * score;
        entry.shard_tokens = (entry.shard_tokens + elapsed * rate).min(burst);
        entry.shard_last = now;
        if entry.shard_tokens >= size as f64 {
            entry.shard_tokens -= size as f64;
            return Ok(());
        }
        {
            let mut metrics = PEER_METRICS.lock().unwrap();
            if let Some(pm) = metrics.get_mut(pk) {
                pm.reputation.penalize(0.9);
                update_reputation_metric(pk, pm.reputation.score);
            }
        }
        let until = Instant::now() + Duration::from_secs(*P2P_BAN_SECS);
        entry.banned_until = Some(until);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time error: {e}"))
            .as_secs()
            + *P2P_BAN_SECS as u64;
        ban_store::store()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ban(pk, ts);
        Err(PeerErrorCode::RateLimit)
    }

    /// Verify and handle an incoming message. Unknown peers or bad signatures are dropped.
    pub fn handle_message(
        &self,
        msg: Message,
        addr: Option<SocketAddr>,
        chain: &Arc<Mutex<Blockchain>>,
    ) {
        let bytes = match bincode::serialize(&msg.body) {
            Ok(b) => b,
            Err(_) => return,
        };
        let pk = match VerifyingKey::from_bytes(&msg.pubkey) {
            Ok(p) => p,
            Err(_) => return,
        };
        let sig = match Signature::from_slice(&msg.signature) {
            Ok(s) => s,
            Err(_) => return,
        };
        if pk.verify(&bytes, &sig).is_err() {
            return;
        }

        record_request(&msg.pubkey);

        if let Err(code) = self.check_rate(&msg.pubkey) {
            telemetry_peer_error(code);
            let reason = match code {
                PeerErrorCode::RateLimit => DropReason::RateLimit,
                PeerErrorCode::Banned => DropReason::Blacklist,
                _ => DropReason::Malformed,
            };
            record_drop(&msg.pubkey, reason);
            if matches!(code, PeerErrorCode::RateLimit | PeerErrorCode::Banned) {
                if let Some(peer_addr) = addr {
                    if let Ok(mut a) = self.addrs.lock() {
                        a.remove(&peer_addr);
                    }
                }
                if let Ok(mut auth) = self.authorized.lock() {
                    auth.remove(&msg.pubkey);
                }
            }
            return;
        }

        match msg.body {
            Payload::Handshake(hs) => {
                if hs.proto_version != PROTOCOL_VERSION {
                    telemetry_peer_error(PeerErrorCode::HandshakeVersion);
                    #[cfg(feature = "telemetry")]
                    {
                        crate::telemetry::PEER_REJECTED_TOTAL
                            .with_label_values(&["protocol"])
                            .inc();
                    crate::telemetry::HANDSHAKE_FAIL_TOTAL
                            .with_label_values(&["protocol"])
                            .inc();
                    }
                    record_handshake_fail(&msg.pubkey, HandshakeError::Version);
                    return;
                }
                if (hs.feature_bits & crate::net::REQUIRED_FEATURES)
                    != crate::net::REQUIRED_FEATURES
                {
                    telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                    #[cfg(feature = "telemetry")]
                    {
                    crate::telemetry::HANDSHAKE_FAIL_TOTAL
                            .with_label_values(&["feature"])
                            .inc();
                    }
                    record_handshake_fail(&msg.pubkey, HandshakeError::Other);
                    return;
                }
                if hs.transport != Transport::Tcp && hs.transport != Transport::Quic {
                    telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                    return;
                }
                self.authorize(msg.pubkey);
                if let Some(peer_addr) = addr {
                    self.add(peer_addr);
                    self.map_addr(peer_addr, msg.pubkey);
                    self.set_transport(peer_addr, hs.transport);
                    if let (Some(qaddr), Some(cert)) = (hs.quic_addr, hs.quic_cert.clone()) {
                        self.set_quic(peer_addr, qaddr, cert);
                    }
                }
            }
            Payload::Hello(addrs) => {
                for a in addrs {
                    self.add(a);
                }
            }
            Payload::Tx(tx) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    let _ = bc.submit_transaction(tx);
                }
            }
            Payload::BlobTx(tx) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    let _ = bc.submit_blob_tx(tx);
                }
            }
            Payload::Block(block) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    if (block.index as usize) == bc.chain.len() {
                        let prev = bc.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                        if block.index == 0 || block.previous_hash == prev {
                            let mut new_chain = bc.chain.clone();
                            new_chain.push(block.clone());
                            if bc.import_chain(new_chain.clone()).is_ok() {
                                drop(bc);
                                let msg = Message::new(Payload::Chain(new_chain), &load_net_key());
                                for p in self.list() {
                                    let _ = send_msg(p, &msg);
                                }
                                return;
                            }
                        }
                    }
                }
            }
            Payload::Chain(new_chain) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Ok(mut bc) = chain.lock() {
                    if new_chain.len() > bc.chain.len() {
                        #[cfg(feature = "telemetry")]
                        let start = Instant::now();
                        let _ = bc.import_chain(new_chain);
                        #[cfg(feature = "telemetry")]
                        observer::observe_convergence(start);
                    }
                }
            }
            Payload::BlobChunk(chunk) => {
                if !self.is_authorized(&msg.pubkey) {
                    return;
                }
                if let Err(code) = self.check_shard_rate(&msg.pubkey, chunk.data.len()) {
                    telemetry_peer_error(code);
                    let reason = match code {
                        PeerErrorCode::RateLimit => DropReason::RateLimit,
                        PeerErrorCode::Banned => DropReason::Blacklist,
                        _ => DropReason::Malformed,
                    };
                    record_drop(&msg.pubkey, reason);
                    if matches!(code, PeerErrorCode::RateLimit | PeerErrorCode::Banned) {
                        if let Some(peer_addr) = addr {
                            if let Ok(mut a) = self.addrs.lock() {
                                a.remove(&peer_addr);
                            }
                        }
                        if let Ok(mut auth) = self.authorized.lock() {
                            auth.remove(&msg.pubkey);
                        }
                    }
                    return;
                }
                let key = format!("chunk/{}/{}", hex::encode(chunk.root), chunk.index);
                let _ = CHUNK_DB.lock().unwrap().try_insert(&key, chunk.data);
            }
            Payload::Reputation(entries) => {
                if crate::compute_market::scheduler::reputation_gossip_enabled() {
                    for e in entries {
                        let applied = crate::compute_market::scheduler::merge_reputation(
                            &e.provider_id,
                            e.reputation_score,
                            e.epoch,
                        );
                        #[cfg(feature = "telemetry")]
                        {
                            crate::telemetry::REPUTATION_GOSSIP_TOTAL
                                .with_label_values(&[if applied { "applied" } else { "ignored" }])
                                .inc();
                        }
                    }
                }
            }
        }
    }
}

struct PeerState {
    count: u32,
    last: Instant,
    banned_until: Option<Instant>,
    shard_tokens: f64,
    shard_last: Instant,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    RateLimit,
    Malformed,
    Blacklist,
    Duplicate,
    Other,
}

impl DropReason {
    fn as_str(&self) -> &'static str {
        match self {
            DropReason::RateLimit => "rate_limit",
            DropReason::Malformed => "malformed",
            DropReason::Blacklist => "blacklist",
            DropReason::Duplicate => "duplicate",
            DropReason::Other => "other",
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HandshakeError {
    Tls,
    Version,
    Timeout,
    Certificate,
    Other,
}

impl HandshakeError {
    fn as_str(&self) -> &'static str {
        match self {
            HandshakeError::Tls => "tls",
            HandshakeError::Version => "version",
            HandshakeError::Timeout => "timeout",
            HandshakeError::Certificate => "certificate",
            HandshakeError::Other => "other",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    pub score: f64,
    #[serde(skip, default = "instant_now")]
    last_decay: Instant,
}

impl Default for PeerReputation {
    fn default() -> Self {
        Self {
            score: 1.0,
            last_decay: Instant::now(),
        }
    }
}

impl PeerReputation {
    fn decay(&mut self, rate: f64) {
        let elapsed = self.last_decay.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let factor = (-rate * elapsed).exp();
            self.score = (self.score * factor).max(0.1);
            self.last_decay = Instant::now();
        }
    }

    fn penalize(&mut self, penalty: f64) {
        self.score = (self.score * penalty).max(0.1);
    }
}

fn instant_now() -> Instant {
    Instant::now()
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct PeerMetrics {
    pub requests: u64,
    pub bytes_sent: u64,
    #[serde(default)]
    pub drops: HashMap<DropReason, u64>,
    #[serde(default)]
    pub handshake_fail: HashMap<HandshakeError, u64>,
    #[serde(default)]
    pub reputation: PeerReputation,
    #[serde(default)]
    pub last_updated: u64,
}

#[derive(Copy, Clone)]
enum PeerErrorCode {
    HandshakeVersion,
    HandshakeFeature,
    RateLimit,
    Banned,
}

#[allow(dead_code)]
impl PeerErrorCode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::HandshakeVersion => "1000",
            Self::HandshakeFeature => "1001",
            Self::RateLimit => "2000",
            Self::Banned => "2001",
        }
    }
}

fn telemetry_peer_error(code: PeerErrorCode) {
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::PEER_ERROR_TOTAL
            .with_label_values(&[code.as_str()])
            .inc();
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = code;
}

pub(crate) fn record_send(addr: SocketAddr, bytes: usize) {
    if let Some(pk) = ADDR_MAP.lock().unwrap().get(&addr).copied() {
        let mut map = PEER_METRICS.lock().unwrap();
        if let Some(mut entry) = map.swap_remove(&pk) {
            entry.bytes_sent += bytes as u64;
            entry.last_updated = now_secs();
            broadcast_metrics(&pk, &entry);
            map.insert(pk, entry);
            update_active_gauge(map.len());
        } else {
            let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
            if map.len() == max {
                if let Some((old, _)) = map.swap_remove_index(0) {
                    #[cfg(feature = "telemetry")]
                    {
                        remove_peer_metrics(&old);
                        if crate::telemetry::should_log("p2p") {
                            let id = hex::encode(old);
                            tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                        }
                    }
                }
            }
            let mut entry = PeerMetrics::default();
            entry.bytes_sent += bytes as u64;
            entry.last_updated = now_secs();
            map.insert(pk, entry);
            update_active_gauge(map.len());
        }
        #[cfg(feature = "telemetry")]
        {
            if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                let id = hex::encode(pk);
                crate::telemetry::PEER_BYTES_SENT_TOTAL
                    .with_label_values(&[id.as_str()])
                    .inc_by(bytes as u64);
            }
        }
    }
}

fn record_request(pk: &[u8; 32]) {
    let mut map = PEER_METRICS.lock().unwrap();
    if let Some(mut entry) = map.swap_remove(pk) {
        entry.requests += 1;
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some((old, _)) = map.swap_remove_index(0) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&old);
                    if crate::telemetry::should_log("p2p") {
                        let id = hex::encode(old);
                        tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        entry.requests += 1;
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = hex::encode(pk);
            crate::telemetry::PEER_REQUEST_TOTAL
                .with_label_values(&[id.as_str()])
                .inc();
        }
    }
}

fn record_drop(pk: &[u8; 32], reason: DropReason) {
    let reason = if TRACK_DROP_REASONS.load(Ordering::Relaxed) {
        reason
    } else {
        DropReason::Other
    };
    let mut map = PEER_METRICS.lock().unwrap();
    if let Some(mut entry) = map.swap_remove(pk) {
        *entry.drops.entry(reason).or_default() += 1;
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some((old, _)) = map.swap_remove_index(0) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&old);
                    if crate::telemetry::should_log("p2p") {
                        let id = hex::encode(old);
                        tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        *entry.drops.entry(reason).or_default() += 1;
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = hex::encode(pk);
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), reason.as_str()])
                .inc();
        }
    }
}

fn record_handshake_fail(pk: &[u8; 32], reason: HandshakeError) {
    if !TRACK_HANDSHAKE_FAIL.load(Ordering::Relaxed) {
        return;
    }
    let mut map = PEER_METRICS.lock().unwrap();
    if let Some(mut entry) = map.swap_remove(pk) {
        *entry.handshake_fail.entry(reason).or_default() += 1;
        entry.reputation.penalize(0.95);
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some((old, _)) = map.swap_remove_index(0) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&old);
                    if crate::telemetry::should_log("p2p") {
                        let id = hex::encode(old);
                        tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        entry.handshake_fail.insert(reason, 1);
        entry.reputation.penalize(0.95);
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = hex::encode(pk);
            crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
                .with_label_values(&[id.as_str(), reason.as_str()])
                .inc();
            crate::telemetry::HANDSHAKE_FAIL_TOTAL
                .with_label_values(&[reason.as_str()])
                .inc();
        }
    }
}

#[cfg(all(feature = "telemetry", feature = "quic"))]
pub(crate) fn record_handshake_fail_addr(addr: SocketAddr, reason: HandshakeError) {
    if let Some(pk) = ADDR_MAP.lock().unwrap().get(&addr).copied() {
        record_handshake_fail(&pk, reason);
    }
}

pub fn simulate_handshake_fail(pk: [u8; 32], reason: HandshakeError) {
    record_handshake_fail(&pk, reason);
}

fn update_reputation_metric(pk: &[u8; 32], score: f64) {
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = hex::encode(pk);
            crate::telemetry::PEER_REPUTATION_SCORE
                .with_label_values(&[id.as_str()])
                .set(score);
        }
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = (pk, score);
}

pub fn reset_peer_metrics(pk: &[u8; 32]) -> bool {
    let mut map = PEER_METRICS.lock().unwrap();
    if let Some(entry) = map.get_mut(pk) {
        *entry = PeerMetrics::default();
        entry.last_updated = now_secs();
        #[cfg(feature = "telemetry")]
        {
            if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                remove_peer_metrics(pk);
                update_reputation_metric(pk, 1.0);
                let id = hex::encode(pk);
                crate::telemetry::PEER_STATS_RESET_TOTAL
                    .with_label_values(&[id.as_str()])
                    .inc();
                if crate::telemetry::should_log("p2p") {
                    tracing::info!(peer = id.as_str(), "reset_peer_metrics");
                }
            }
        }
        true
    } else {
        false
    }
}

pub fn rotate_peer_key(old: &[u8; 32], new: [u8; 32]) -> bool {
    let mut map = PEER_METRICS.lock().unwrap();
    if let Some((_, metrics)) = map.shift_remove_entry(old) {
        map.insert(new, metrics);
        update_active_gauge(map.len());
        drop(map);
        let mut addr_map = ADDR_MAP.lock().unwrap();
        for val in addr_map.values_mut() {
            if *val == *old {
                *val = new;
            }
        }
        drop(addr_map);
        {
            let path = KEY_HISTORY_PATH.lock().unwrap().clone();
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            let entry = json!({
                "old": hex::encode(old),
                "new": hex::encode(new),
                "ts": now_secs(),
            });
            let _ = writeln!(file, "{}", entry.to_string());
        }
        ROTATED_KEYS.lock().unwrap().insert(new, *old);
        true
    } else {
        false
    }
}

pub fn peer_stats(pk: &[u8; 32]) -> Option<PeerMetrics> {
    let res = PEER_METRICS.lock().unwrap().get(pk).cloned();
    #[cfg(feature = "telemetry")]
    if res.is_some() && EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let id = hex::encode(pk);
        crate::telemetry::PEER_STATS_QUERY_TOTAL
            .with_label_values(&[id.as_str()])
            .inc();
    }
    res
}

pub fn export_peer_stats(pk: &[u8; 32], name: &str) -> std::io::Result<bool> {
    let res = (|| {
        let rel = Path::new(name);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid path",
            ));
        }
        let dir = METRICS_EXPORT_DIR.lock().unwrap().clone();
        std::fs::create_dir_all(&dir)?;
        let path = Path::new(&dir).join(rel);
        let metrics = {
            let map = PEER_METRICS.lock().unwrap();
            map.get(pk).cloned()
        };
        let metrics =
            metrics.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "peer"))?;
        let json = serde_json::to_vec(&metrics)?;
        let mut tmp = NamedTempFile::new_in(&dir)?;
        tmp.write_all(&json)?;
        tmp.flush()?;
        let overwritten = path.exists();
        tmp.persist(&path)?;
        Ok(overwritten)
    })();

    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let label = if res.is_ok() { "ok" } else { "error" };
            crate::telemetry::PEER_STATS_EXPORT_TOTAL
                .with_label_values(&[label])
                .inc();
        }
    }

    res
}

pub fn export_all_peer_stats(name: &str) -> std::io::Result<bool> {
    let res = (|| {
        let rel = Path::new(name);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid path",
            ));
        }
        let dir = METRICS_EXPORT_DIR.lock().unwrap().clone();
        std::fs::create_dir_all(&dir)?;
        let path = Path::new(&dir).join(rel);
        let mut tmp = NamedTempFile::new_in(&dir)?;
        {
            let mut tar = Builder::new(tmp.as_file_mut());
            let map = PEER_METRICS.lock().unwrap();
            for (pk, m) in map.iter() {
                let id = hex::encode(pk);
                let data = serde_json::to_vec(m)?;
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_cksum();
                tar.append_data(&mut header, format!("{id}.json"), data.as_slice())?;
            }
            tar.finish()?;
        }
        let overwritten = path.exists();
        tmp.persist(&path)?;
        Ok(overwritten)
    })();

    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let label = if res.is_ok() { "ok" } else { "error" };
            crate::telemetry::PEER_STATS_EXPORT_TOTAL
                .with_label_values(&[label])
                .inc();
        }
    }

    res
}

#[derive(Serialize, Deserialize)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: PeerMetrics,
}

pub fn peer_stats_all(offset: usize, limit: usize) -> Vec<PeerStat> {
    PEER_METRICS
        .lock()
        .unwrap()
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(pk, m)| PeerStat {
            peer_id: hex::encode(pk),
            metrics: m.clone(),
        })
        .collect()
}

#[cfg(feature = "telemetry")]
fn remove_peer_metrics(pk: &[u8; 32]) {
    let id = hex::encode(pk);
    let _ = crate::telemetry::PEER_REQUEST_TOTAL.remove_label_values(&[id.as_str()]);
    let _ = crate::telemetry::PEER_BYTES_SENT_TOTAL.remove_label_values(&[id.as_str()]);
    for reason in DROP_REASON_VARIANTS {
        let _ =
            crate::telemetry::PEER_DROP_TOTAL.remove_label_values(&[id.as_str(), reason.as_str()]);
    }
    for reason in HANDSHAKE_ERROR_VARIANTS {
        let _ = crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
            .remove_label_values(&[id.as_str(), reason.as_str()]);
    }
    let _ = crate::telemetry::PEER_REPUTATION_SCORE.remove_label_values(&[id.as_str()]);
}

#[cfg(not(feature = "telemetry"))]
fn remove_peer_metrics(_pk: &[u8; 32]) {}

pub fn record_ip_drop(ip: &SocketAddr) {
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = ip.to_string();
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), DropReason::Duplicate.as_str()])
                .inc();
        }
    }
}

fn peer_db_path() -> PathBuf {
    std::env::var("TB_PEER_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("peers.txt")
        })
}

#[derive(Clone, Serialize, Deserialize)]
struct QuicEndpoint {
    addr: SocketAddr,
    cert: Vec<u8>,
}

fn quic_peer_db_path() -> PathBuf {
    std::env::var("TB_QUIC_PEER_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("quic_peers.txt")
        })
}

fn load_quic_peers() -> HashMap<SocketAddr, QuicEndpoint> {
    use base64::Engine;
    let mut map = HashMap::new();
    if let Ok(data) = fs::read_to_string(quic_peer_db_path()) {
        for line in data.lines() {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() == 3 {
                if let (Ok(tcp), Ok(quic)) = (parts[0].parse(), parts[1].parse()) {
                    if let Ok(cert) = base64::engine::general_purpose::STANDARD.decode(parts[2]) {
                        map.insert(tcp, QuicEndpoint { addr: quic, cert });
                    }
                }
            }
        }
    }
    map
}

fn persist_quic_peers(map: &HashMap<SocketAddr, QuicEndpoint>) {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    let path = quic_peer_db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = map
        .iter()
        .map(|(tcp, info)| format!("{tcp},{},{}", info.addr, B64.encode(&info.cert)))
        .collect();
    lines.sort();
    let _ = fs::write(path, lines.join("\n"));
}

fn chunk_db_path() -> PathBuf {
    std::env::var("TB_CHUNK_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("chunks")
        })
}

static CHUNK_DB: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = chunk_db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    Mutex::new(SimpleDb::open(path.to_str().unwrap()))
});

fn persist_peers(set: &HashSet<SocketAddr>) {
    let path = peer_db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut addrs: Vec<String> = set.iter().map(|a| a.to_string()).collect();
    addrs.sort();
    let _ = fs::write(path, addrs.join("\n"));
}

pub fn known_peers() -> Vec<SocketAddr> {
    if let Ok(data) = fs::read_to_string(peer_db_path()) {
        data.lines().filter_map(|l| l.parse().ok()).collect()
    } else {
        Vec::new()
    }
}

static P2P_MAX_PER_SEC: Lazy<AtomicU32> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_MAX_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    AtomicU32::new(val)
});

static P2P_BAN_SECS: Lazy<u64> = Lazy::new(|| {
    std::env::var("TB_P2P_BAN_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
});

static P2P_SHARD_RATE: Lazy<f64> = Lazy::new(|| {
    std::env::var("TB_P2P_SHARD_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256_000.0)
});

static P2P_SHARD_BURST: Lazy<u64> = Lazy::new(|| {
    std::env::var("TB_P2P_SHARD_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000)
});

static PEER_METRICS: Lazy<Mutex<IndexMap<[u8; 32], PeerMetrics>>> =
    Lazy::new(|| Mutex::new(IndexMap::new()));

static ADDR_MAP: Lazy<Mutex<HashMap<SocketAddr, [u8; 32]>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static MAX_PEER_METRICS: AtomicUsize = AtomicUsize::new(1024);
static EXPORT_PEER_METRICS: AtomicBool = AtomicBool::new(true);
static TRACK_DROP_REASONS: AtomicBool = AtomicBool::new(true);
static TRACK_HANDSHAKE_FAIL: AtomicBool = AtomicBool::new(true);
static PEER_REPUTATION_DECAY: AtomicU64 = AtomicU64::new(f64::to_bits(0.01));
static PEER_METRICS_PATH: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new("state/peer_metrics.json".into()));
static METRICS_EXPORT_DIR: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("state".into()));
static PEER_METRICS_RETENTION: AtomicU64 = AtomicU64::new(7 * 24 * 60 * 60);
static PEER_METRICS_COMPRESS: AtomicBool = AtomicBool::new(false);
static KEY_HISTORY_PATH: Lazy<Mutex<String>> = Lazy::new(|| {
    let path = std::env::var("TB_PEER_KEY_HISTORY_PATH")
        .unwrap_or_else(|_| "state/peer_key_history.log".into());
    Mutex::new(path)
});
static ROTATED_KEYS: Lazy<Mutex<HashMap<[u8; 32], [u8; 32]>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const PEER_METRICS_VERSION: u32 = 1;

#[derive(Clone, Serialize)]
pub struct PeerSnapshot {
    pub peer_id: String,
    pub metrics: PeerMetrics,
}

static METRIC_TX: Lazy<broadcast::Sender<PeerSnapshot>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(1024);
    tx
});

pub struct MetricsReceiver {
    inner: broadcast::Receiver<PeerSnapshot>,
}

impl MetricsReceiver {
    pub async fn recv(&mut self) -> Result<PeerSnapshot, broadcast::error::RecvError> {
        self.inner.recv().await
    }
}

impl Drop for MetricsReceiver {
    fn drop(&mut self) {
        ACTIVE_SUBSCRIBERS.fetch_sub(1, Ordering::Relaxed);
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::PEER_METRICS_SUBSCRIBERS
                .set(ACTIVE_SUBSCRIBERS.load(Ordering::Relaxed) as i64);
        }
    }
}

static ACTIVE_SUBSCRIBERS: AtomicUsize = AtomicUsize::new(0);
static DROPPED_FRAMES: AtomicU64 = AtomicU64::new(0);

pub fn subscribe_peer_metrics() -> MetricsReceiver {
    ACTIVE_SUBSCRIBERS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::PEER_METRICS_SUBSCRIBERS
            .set(ACTIVE_SUBSCRIBERS.load(Ordering::Relaxed) as i64);
    }
    MetricsReceiver {
        inner: METRIC_TX.subscribe(),
    }
}

pub fn broadcast_metrics(pk: &[u8; 32], m: &PeerMetrics) {
    let snap = PeerSnapshot {
        peer_id: hex::encode(pk),
        metrics: m.clone(),
    };
    if METRIC_TX.send(snap).is_err() {
        DROPPED_FRAMES.fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "telemetry")]
        crate::telemetry::PEER_METRICS_DROPPED.inc();
    }
}

const DROP_REASON_VARIANTS: &[DropReason] = &[
    DropReason::RateLimit,
    DropReason::Malformed,
    DropReason::Blacklist,
    DropReason::Duplicate,
    DropReason::Other,
];

const HANDSHAKE_ERROR_VARIANTS: &[HandshakeError] = &[
    HandshakeError::Tls,
    HandshakeError::Version,
    HandshakeError::Timeout,
    HandshakeError::Certificate,
    HandshakeError::Other,
];

pub fn set_max_peer_metrics(max: usize) {
    MAX_PEER_METRICS.store(max, Ordering::Relaxed);
}

pub fn set_peer_metrics_export(val: bool) {
    EXPORT_PEER_METRICS.store(val, Ordering::Relaxed);
}

pub fn set_track_drop_reasons(val: bool) {
    TRACK_DROP_REASONS.store(val, Ordering::Relaxed);
}

pub fn set_track_handshake_fail(val: bool) {
    TRACK_HANDSHAKE_FAIL.store(val, Ordering::Relaxed);
}

pub fn set_peer_reputation_decay(rate: f64) {
    PEER_REPUTATION_DECAY.store(rate.to_bits(), Ordering::Relaxed);
}

pub fn set_peer_metrics_path(path: String) {
    *PEER_METRICS_PATH.lock().unwrap() = path;
}

pub fn set_peer_metrics_retention(ttl: u64) {
    PEER_METRICS_RETENTION.store(ttl, Ordering::Relaxed);
}

pub fn set_metrics_export_dir(dir: String) {
    *METRICS_EXPORT_DIR.lock().unwrap() = dir;
}

pub fn set_peer_metrics_compress(val: bool) {
    PEER_METRICS_COMPRESS.store(val, Ordering::Relaxed);
}

pub fn set_p2p_max_per_sec(v: u32) {
    P2P_MAX_PER_SEC.store(v, Ordering::Relaxed);
}

pub fn p2p_max_per_sec() -> u32 {
    P2P_MAX_PER_SEC.load(Ordering::Relaxed)
}

pub fn peer_reputation_decay() -> f64 {
    f64::from_bits(PEER_REPUTATION_DECAY.load(Ordering::Relaxed))
}

#[cfg(feature = "telemetry")]
fn update_active_gauge(len: usize) {
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        crate::telemetry::PEER_METRICS_ACTIVE.set(len as i64);
    }
}

#[cfg(not(feature = "telemetry"))]
fn update_active_gauge(_len: usize) {}

#[cfg(feature = "telemetry")]
fn register_peer_metrics(pk: &[u8; 32], m: &PeerMetrics) {
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let id = hex::encode(pk);
        crate::telemetry::PEER_REQUEST_TOTAL
            .with_label_values(&[id.as_str()])
            .inc_by(m.requests);
        crate::telemetry::PEER_BYTES_SENT_TOTAL
            .with_label_values(&[id.as_str()])
            .inc_by(m.bytes_sent);
        for (r, c) in &m.drops {
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), r.as_str()])
                .inc_by(*c);
        }
        for (r, c) in &m.handshake_fail {
            crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
                .with_label_values(&[id.as_str(), r.as_str()])
                .inc_by(*c);
        }
        update_reputation_metric(pk, m.reputation.score);
    }
}

#[cfg(not(feature = "telemetry"))]
fn register_peer_metrics(_pk: &[u8; 32], _m: &PeerMetrics) {}

pub fn persist_peer_metrics() -> std::io::Result<()> {
    let path = PEER_METRICS_PATH.lock().unwrap().clone();
    let compress = PEER_METRICS_COMPRESS.load(Ordering::Relaxed);
    let map = PEER_METRICS.lock().unwrap();
    let peers: Vec<_> = map
        .iter()
        .map(|(pk, m)| PersistEntry {
            peer_id: hex::encode(pk),
            metrics: m.clone(),
        })
        .collect();
    let data = PersistFile {
        version: PEER_METRICS_VERSION,
        peers,
    };
    let json = serde_json::to_vec(&data)?;
    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = format!("{}.tmp", path);
    if compress {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&json)?;
        fs::write(&tmp, enc.finish()?)?;
    } else {
        fs::write(&tmp, &json)?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn load_peer_metrics() {
    let path = PEER_METRICS_PATH.lock().unwrap().clone();
    let ttl = PEER_METRICS_RETENTION.load(Ordering::Relaxed);
    let compress = PEER_METRICS_COMPRESS.load(Ordering::Relaxed);
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(_) => return,
    };
    let bytes = if compress {
        let mut dec = flate2::read::GzDecoder::new(&data[..]);
        let mut out = Vec::new();
        if dec.read_to_end(&mut out).is_err() {
            return;
        }
        out
    } else {
        data
    };
    if let Ok(file) = serde_json::from_slice::<PersistFile>(&bytes) {
        if file.version != PEER_METRICS_VERSION {
            return;
        }
        let now = now_secs();
        let mut map = PEER_METRICS.lock().unwrap();
        #[cfg(feature = "telemetry")]
        let export = EXPORT_PEER_METRICS.load(Ordering::Relaxed);
        map.clear();
        for entry in file.peers {
            if now.saturating_sub(entry.metrics.last_updated) > ttl {
                continue;
            }
            if let Ok(bytes) = hex::decode(&entry.peer_id) {
                if let Ok(pk) = <[u8; 32]>::try_from(bytes.as_slice()) {
                    let m = entry.metrics;
                    if export {
                        register_peer_metrics(&pk, &m);
                    }
                    map.insert(pk, m);
                }
            }
        }
        update_active_gauge(map.len());
    }
}

pub fn clear_peer_metrics() {
    let mut map = PEER_METRICS.lock().unwrap();
    #[cfg(feature = "telemetry")]
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        for pk in map.keys().cloned().collect::<Vec<_>>() {
            remove_peer_metrics(&pk);
        }
    }
    map.clear();
    update_active_gauge(0);
}

#[derive(Serialize, Deserialize)]
struct PersistEntry {
    peer_id: String,
    metrics: PeerMetrics,
}

#[derive(Serialize, Deserialize)]
struct PersistFile {
    version: u32,
    peers: Vec<PersistEntry>,
}
