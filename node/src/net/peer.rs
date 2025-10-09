use super::{
    load_net_key, overlay_peer_from_bytes, overlay_peer_to_base58, send_msg, PROTOCOL_VERSION,
};
use crate::config::AggregatorConfig;
#[cfg(feature = "telemetry")]
use crate::consensus::observer;
use crate::net::message::{Message, Payload};
#[cfg(feature = "quic")]
use crate::p2p::handshake::validate_quic_certificate;
use crate::p2p::handshake::Transport;
use crate::simple_db::{names, SimpleDb};
use crate::Blockchain;
use concurrency::{Lazy, MutexExt};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{
    json::{self, Value},
    Error as SerializationError,
};
use foundation_serialization::{Deserialize, Serialize};
use hex;
use indexmap::IndexMap;
use rand::{rngs::StdRng, seq::SliceRandom};
use runtime::net::lookup_srv;
use runtime::sync::broadcast;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use sys::fs::{FileLockExt, O_NOFOLLOW};

use sys::paths;
use sys::tempfile::{self, Builder as TempBuilder, NamedTempFile};
use tar::Builder;

fn sys_to_io_error(err: sys::error::SysError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

fn json_to_io_error(err: SerializationError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

use super::{ban_store, peer_metrics_store};
#[cfg(feature = "quic")]
use super::{record_peer_certificate, verify_peer_fingerprint};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "telemetry")]
fn log_suspicious(path: &str) {
    let count = SUSPICIOUS_EXPORTS.fetch_add(1, Ordering::Relaxed) + 1;
    if count % 100 == 0 {
        diagnostics::tracing::warn!(%path, "suspicious metrics export attempt count={}", count);
    }
}

#[cfg(not(feature = "telemetry"))]
#[allow(dead_code)]
fn log_suspicious(_path: &str) {}

fn overlay_peer_label(pk: &[u8; 32]) -> String {
    overlay_peer_from_bytes(pk)
        .map(|peer| overlay_peer_to_base58(&peer))
        .unwrap_or_else(|_| hex::encode(pk))
}

/// Gossiped reputation update.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ReputationUpdate {
    /// Provider identifier being scored.
    pub provider_id: String,
    /// Reputation score for the provider.
    pub reputation_score: i64,
    /// Epoch the score was computed in.
    pub epoch: u64,
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
        let mut guard = self.addrs.guard();
        guard.insert(addr);
        persist_peers(&guard);
        let mut map = self.transports.guard();
        map.entry(addr).or_insert(Transport::Tcp);
        let q = self.quic.guard();
        if !q.contains_key(&addr) {
            persist_quic_peers(&q);
        }
    }

    /// Remove a peer from the set.
    pub fn remove(&self, addr: SocketAddr) {
        let mut guard = self.addrs.guard();
        guard.remove(&addr);
        persist_peers(&guard);
        let mut map = self.transports.guard();
        map.remove(&addr);
    }

    /// Clear all peers from the set.
    pub fn clear(&self) {
        let mut guard = self.addrs.guard();
        guard.clear();
        persist_peers(&guard);
        let mut map = self.transports.guard();
        map.clear();
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.addrs.guard().iter().copied().collect()
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
        let addrs = self.addrs.guard();
        let transports = self.transports.guard();
        let quic = self.quic.guard();
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
        {
            let mut m = ADDR_MAP.guard();
            m.insert(addr, pk);
        }
        #[cfg(feature = "quic")]
        super::quic_stats::record_address(&pk, addr);
        let mut metrics = peer_metrics_guard();
        if let Some(val) = metrics.swap_remove(&pk) {
            metrics.insert(pk, val);
            update_active_gauge(metrics.len());
            return;
        }
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if metrics.len() == max {
            if let Some(_old) = evict_lru(&mut metrics) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&_old);
                    if crate::telemetry::should_log("p2p") {
                        let id = overlay_peer_label(&_old);
                        diagnostics::tracing::info!(peer = id.as_str(), "evict_peer_metrics");
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
        let mut map = self.transports.guard();
        map.insert(addr, transport);
    }

    /// Record QUIC endpoint info for `addr`.
    pub fn set_quic(&self, addr: SocketAddr, quic_addr: SocketAddr, cert: Vec<u8>) {
        let mut map = self.quic.guard();
        map.insert(
            addr,
            QuicEndpoint {
                addr: quic_addr,
                cert,
            },
        );
        persist_quic_peers(&map);
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
        self.authorized.guard().insert(pk);
        let mut ids = PEER_IDENTITIES.guard();
        ids.entry(pk).or_insert(PeerIdentity {
            peer_id: pk,
            public_key: pk,
            old_key: None,
            rotated_at: None,
        });
    }

    fn is_authorized(&self, pk: &[u8; 32]) -> bool {
        self.authorized.guard().contains(pk)
    }

    fn check_rate(&self, pk: &[u8; 32]) -> Result<(), PeerErrorCode> {
        if ban_store_guard().is_banned(pk) {
            return Err(PeerErrorCode::Banned);
        }
        let mut map = self.states.guard();
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
        let limit = p2p_max_per_sec();
        let allowed = {
            let mut metrics = peer_metrics_guard();
            let pm = metrics.entry(*pk).or_insert_with(PeerMetrics::default);
            pm.reputation.decay(peer_reputation_decay());
            pm.last_updated = now_secs();
            let score = pm.reputation.score;
            update_reputation_metric(pk, score);
            let mut allowed = ((limit as f64) * score).floor() as u32;
            if limit > 0 {
                allowed = allowed.max(1);
            }
            allowed
        };
        if entry.count > allowed {
            {
                let mut metrics = peer_metrics_guard();
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
            ban_store_guard().ban(pk, ts);
            #[cfg(feature = "telemetry")]
            {
                let id = overlay_peer_label(pk);
                crate::telemetry::P2P_REQUEST_LIMIT_HITS_TOTAL
                    .with_label_values(&[id.as_str()])
                    .inc();
            }
            return Err(PeerErrorCode::RateLimit);
        }
        Ok(())
    }

    fn check_shard_rate(&self, pk: &[u8; 32], size: usize) -> Result<(), PeerErrorCode> {
        let mut map = self.states.guard();
        let entry = map.entry(*pk).or_insert(PeerState {
            count: 0,
            last: Instant::now(),
            banned_until: None,
            shard_tokens: *P2P_SHARD_BURST as f64,
            shard_last: Instant::now(),
        });
        let score = {
            let mut metrics = peer_metrics_guard();
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
            let mut metrics = peer_metrics_guard();
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
        ban_store_guard().ban(pk, ts);
        #[cfg(feature = "telemetry")]
        {
            let id = overlay_peer_label(pk);
            crate::telemetry::P2P_REQUEST_LIMIT_HITS_TOTAL
                .with_label_values(&[id.as_str()])
                .inc();
        }
        Err(PeerErrorCode::RateLimit)
    }

    /// Verify and handle an incoming message. Unknown peers or bad signatures are dropped.
    pub fn handle_message(
        &self,
        msg: Message,
        addr: Option<SocketAddr>,
        chain: &Arc<StdMutex<Blockchain>>,
    ) {
        let bytes = match bincode::serialize(&msg.body) {
            Ok(b) => b,
            Err(_) => return,
        };
        let pk = match VerifyingKey::from_bytes(&msg.pubkey) {
            Ok(p) => p,
            Err(_) => return,
        };
        let sig_bytes: [u8; 64] = match msg.signature.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        let sig = Signature::from_bytes(&sig_bytes);
        if pk.verify(&bytes, &sig).is_err() {
            return;
        }

        let mut peer_key = msg.pubkey;
        if let Some((new, revoke)) = ROTATED_KEYS.guard().get(&peer_key).copied() {
            if now_secs() > revoke {
                return;
            }
            peer_key = new;
        }

        #[cfg(feature = "quic")]
        let msg_fingerprint = msg.cert_fingerprint.as_ref().and_then(|fp| {
            if fp.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(fp);
                Some(arr)
            } else {
                None
            }
        });
        #[cfg(feature = "quic")]
        if !matches!(msg.body, Payload::Handshake(_))
            && !verify_peer_fingerprint(&peer_key, msg_fingerprint.as_ref().map(|fp| fp))
        {
            record_drop(&peer_key, DropReason::Malformed);
            return;
        }

        record_request(&peer_key);

        if is_throttled(&peer_key) {
            if let Some(_m) = peer_stats(&peer_key) {
                #[cfg(feature = "telemetry")]
                if let Some(reason) = _m.throttle_reason.as_deref() {
                    crate::telemetry::PEER_BACKPRESSURE_DROPPED_TOTAL
                        .with_label_values(&[reason])
                        .inc();
                }
            }
            record_drop(&peer_key, DropReason::TooBusy);
            return;
        }

        if let Err(code) = self.check_rate(&peer_key) {
            telemetry_peer_error(code);
            let reason = match code {
                PeerErrorCode::RateLimit => DropReason::RateLimit,
                PeerErrorCode::Banned => DropReason::Blacklist,
                _ => DropReason::Malformed,
            };
            record_drop(&peer_key, reason);
            if matches!(code, PeerErrorCode::RateLimit | PeerErrorCode::Banned) {
                if let Some(peer_addr) = addr {
                    let mut a = self.addrs.guard();
                    a.remove(&peer_addr);
                }
                self.authorized.guard().remove(&peer_key);
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
                    record_handshake_fail(&peer_key, HandshakeError::Version);
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
                    record_handshake_fail(&peer_key, HandshakeError::Other);
                    return;
                }
                if hs.transport != Transport::Tcp && hs.transport != Transport::Quic {
                    telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                    return;
                }
                #[cfg(feature = "quic")]
                let validated_cert = match validate_quic_certificate(&peer_key, &hs) {
                    Ok(v) => v,
                    Err(_) => {
                        telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                        #[cfg(feature = "telemetry")]
                        {
                            crate::telemetry::HANDSHAKE_FAIL_TOTAL
                                .with_label_values(&["certificate"])
                                .inc();
                        }
                        record_handshake_fail(&peer_key, HandshakeError::Certificate);
                        return;
                    }
                };
                self.authorize(peer_key);
                record_handshake_success(&peer_key);
                if let Some(peer_addr) = addr {
                    self.add(peer_addr);
                    self.map_addr(peer_addr, peer_key);
                    self.set_transport(peer_addr, hs.transport);
                    if let (Some(qaddr), Some(cert)) = (hs.quic_addr, hs.quic_cert.clone()) {
                        self.set_quic(peer_addr, qaddr, cert.clone());
                        #[cfg(feature = "quic")]
                        if let Some(vc) = &validated_cert {
                            record_peer_certificate(
                                &peer_key,
                                &vc.provider,
                                cert,
                                vc.fingerprint,
                                vc.previous.clone(),
                            );
                        }
                    }
                }
            }
            Payload::Hello(addrs) => {
                for a in addrs {
                    self.add(a);
                }
            }
            Payload::Tx(tx) => {
                if !self.is_authorized(&peer_key) {
                    return;
                }
                let mut bc = chain.guard();
                let _ = bc.submit_transaction(tx);
            }
            Payload::BlobTx(tx) => {
                if !self.is_authorized(&peer_key) {
                    return;
                }
                let mut bc = chain.guard();
                let _ = bc.submit_blob_tx(tx);
            }
            Payload::Block(shard, block) => {
                if !self.is_authorized(&peer_key) {
                    return;
                }
                if let Ok(peer_id) = crate::net::overlay_peer_from_bytes(&peer_key) {
                    crate::net::register_shard_peer(shard, peer_id);
                }
                let mut bc = chain.guard();
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
            Payload::Chain(new_chain) => {
                if !self.is_authorized(&peer_key) {
                    return;
                }
                let mut bc = chain.guard();
                if new_chain.len() > bc.chain.len() {
                    #[cfg(feature = "telemetry")]
                    let start = Instant::now();
                    let _ = bc.import_chain(new_chain);
                    #[cfg(feature = "telemetry")]
                    observer::observe_convergence(start);
                }
            }
            Payload::BlobChunk(chunk) => {
                if !self.is_authorized(&peer_key) {
                    return;
                }
                if let Err(code) = self.check_shard_rate(&peer_key, chunk.data.len()) {
                    telemetry_peer_error(code);
                    let reason = match code {
                        PeerErrorCode::RateLimit => DropReason::RateLimit,
                        PeerErrorCode::Banned => DropReason::Blacklist,
                        _ => DropReason::Malformed,
                    };
                    record_drop(&peer_key, reason);
                    if matches!(code, PeerErrorCode::RateLimit | PeerErrorCode::Banned) {
                        if let Some(peer_addr) = addr {
                            let mut a = self.addrs.guard();
                            a.remove(&peer_addr);
                        }
                        self.authorized.guard().remove(&peer_key);
                    }
                    return;
                }
                let key = format!("chunk/{}/{}", hex::encode(chunk.root), chunk.index);
                let _ = CHUNK_DB.guard().try_insert(&key, chunk.data);
            }
            Payload::Reputation(entries) => {
                if crate::compute_market::scheduler::reputation_gossip_enabled() {
                    for e in entries {
                        let _applied = crate::compute_market::scheduler::merge_reputation(
                            &e.provider_id,
                            e.reputation_score,
                            e.epoch,
                        );
                        #[cfg(feature = "telemetry")]
                        {
                            crate::telemetry::REPUTATION_GOSSIP_TOTAL
                                .with_label_values(&[if _applied { "applied" } else { "ignored" }])
                                .inc();
                            let latency = now_secs().saturating_sub(e.epoch) as f64;
                            crate::telemetry::REPUTATION_GOSSIP_LATENCY_SECONDS.observe(latency);
                            if !_applied {
                                crate::telemetry::REPUTATION_GOSSIP_FAIL_TOTAL.inc();
                            }
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
    TooBusy,
    Other,
}

impl AsRef<str> for DropReason {
    fn as_ref(&self) -> &str {
        match self {
            DropReason::RateLimit => "rate_limit",
            DropReason::Malformed => "malformed",
            DropReason::Blacklist => "blacklist",
            DropReason::Duplicate => "duplicate",
            DropReason::TooBusy => "too_busy",
            DropReason::Other => "other",
        }
    }
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandshakeError {
    Tls,
    Version,
    Timeout,
    Certificate,
    Other,
}

impl HandshakeError {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            HandshakeError::Tls => "tls",
            HandshakeError::Version => "version",
            HandshakeError::Timeout => "timeout",
            HandshakeError::Certificate => "certificate",
            HandshakeError::Other => "other",
        }
    }
}

/// Stable identity for a peer independent of its current public key.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq, Hash)]
pub struct PeerIdentity {
    /// Logical identifier for the peer.
    pub peer_id: [u8; 32],
    /// Active public key used for message signatures.
    pub public_key: [u8; 32],
    /// Previously active key kept during rotation grace period.
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub old_key: Option<[u8; 32]>,
    /// Rotation timestamp for audit and expiry.
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub rotated_at: Option<u64>,
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
    #[serde(default = "foundation_serialization::defaults::default")]
    pub sends: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub drops: HashMap<DropReason, u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub handshake_fail: HashMap<HandshakeError, u64>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub handshake_success: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub last_handshake_ms: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub tls_errors: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub reputation: PeerReputation,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub last_updated: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub req_avg: f64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub byte_avg: f64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub throttled_until: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub throttle_reason: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub backoff_level: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub sec_start: u64,
    #[serde(skip)]
    pub sec_requests: u64,
    #[serde(skip)]
    pub sec_bytes: u64,
    #[serde(skip)]
    pub breach_count: u32,
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

fn update_peer_rates(pk: &[u8; 32], entry: &mut PeerMetrics, bytes: u64, reqs: u64, now: u64) {
    if entry.sec_start == 0 {
        entry.sec_start = now;
    }
    if now > entry.sec_start {
        entry.req_avg = (entry.req_avg * 4.0 + entry.sec_requests as f64) / 5.0;
        entry.byte_avg = (entry.byte_avg * 4.0 + entry.sec_bytes as f64) / 5.0;
        entry.sec_requests = 0;
        entry.sec_bytes = 0;
        entry.sec_start = now;
    }
    entry.sec_requests += reqs;
    entry.sec_bytes += bytes;
    let req_rate = (entry.req_avg * 4.0 + entry.sec_requests as f64) / 5.0;
    let byte_rate = (entry.byte_avg * 4.0 + entry.sec_bytes as f64) / 5.0;
    if entry.throttled_until != 0 && entry.throttled_until <= now {
        entry.throttled_until = 0;
        entry.throttle_reason = None;
        entry.breach_count = 0;
        entry.backoff_level = 0;
    }
    let mut reason = None;
    if req_rate > p2p_max_per_sec() as f64 {
        reason = Some("requests");
    } else if byte_rate > p2p_max_bytes_per_sec() as f64 {
        reason = Some("bandwidth");
    }
    if let Some(r) = reason {
        entry.breach_count += 1;
        if entry.breach_count >= 3 && entry.throttled_until == 0 {
            let dur = *P2P_THROTTLE_SECS << entry.backoff_level.min(5);
            entry.throttled_until = now + dur;
            entry.throttle_reason = Some(r.to_string());
            entry.backoff_level = entry.backoff_level.saturating_add(1);
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::PEER_THROTTLE_TOTAL
                    .with_label_values(&[r])
                    .inc();
                crate::telemetry::PEER_BACKPRESSURE_ACTIVE_TOTAL
                    .with_label_values(&[r])
                    .inc();
                if crate::telemetry::should_log("p2p") {
                    let id = overlay_peer_label(pk);
                    diagnostics::tracing::warn!(
                        peer = id.as_str(),
                        reason = r,
                        duration = dur,
                        "peer_throttled"
                    );
                }
            }
            entry.reputation.penalize(0.9);
            update_reputation_metric(pk, entry.reputation.score);
        }
    } else {
        entry.breach_count = 0;
    }
}

pub(crate) fn record_send(addr: SocketAddr, bytes: usize) {
    if let Some(pk) = ADDR_MAP.guard().get(&addr).copied() {
        let mut map = peer_metrics_guard();
        maybe_consolidate(&mut map);
        let now = now_secs();
        if let Some(mut entry) = map.swap_remove(&pk) {
            entry.bytes_sent += bytes as u64;
            entry.sends += 1;
            update_peer_rates(&pk, &mut entry, bytes as u64, 0, now);
            entry.last_updated = now;
            broadcast_metrics(&pk, &entry);
            #[cfg(feature = "telemetry")]
            let sends = entry.sends;
            map.insert(pk, entry);
            if let Some(st) = map.get(&pk) {
                persist_snapshot(&pk, st);
            }
            update_active_gauge(map.len());
            update_memory_usage(map.len());
            #[cfg(feature = "telemetry")]
            {
                if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                    let sample = PEER_METRICS_SAMPLE_RATE.load(Ordering::Relaxed);
                    if sample <= 1 || sends % sample == 0 {
                        let id = overlay_peer_label(&pk);
                        crate::telemetry::PEER_BYTES_SENT_TOTAL
                            .with_label_values(&[id.as_str()])
                            .inc_by(bytes as u64 * sample as u64);
                    }
                }
            }
        } else {
            let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
            if map.len() == max {
                if let Some(_old) = evict_lru(&mut map) {
                    #[cfg(feature = "telemetry")]
                    {
                        remove_peer_metrics(&_old);
                        if crate::telemetry::should_log("p2p") {
                            let id = overlay_peer_label(&_old);
                            diagnostics::tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                        }
                    }
                }
            }
            let mut entry = PeerMetrics::default();
            entry.bytes_sent += bytes as u64;
            entry.sends += 1;
            update_peer_rates(&pk, &mut entry, bytes as u64, 0, now);
            entry.last_updated = now;
            #[cfg(feature = "telemetry")]
            let sends = entry.sends;
            map.insert(pk, entry);
            update_active_gauge(map.len());
            update_memory_usage(map.len());
            #[cfg(feature = "telemetry")]
            {
                if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                    let sample = PEER_METRICS_SAMPLE_RATE.load(Ordering::Relaxed);
                    if sample <= 1 || sends % sample == 0 {
                        let id = overlay_peer_label(&pk);
                        crate::telemetry::PEER_BYTES_SENT_TOTAL
                            .with_label_values(&[id.as_str()])
                            .inc_by(bytes as u64 * sample as u64);
                    }
                }
            }
        }
    }
}

pub fn record_request(pk: &[u8; 32]) {
    let mut map = peer_metrics_guard();
    maybe_consolidate(&mut map);
    let now = now_secs();
    if let Some(mut entry) = map.swap_remove(pk) {
        entry.requests += 1;
        update_peer_rates(pk, &mut entry, 0, 1, now);
        entry.last_updated = now;
        broadcast_metrics(pk, &entry);
        #[cfg(feature = "telemetry")]
        let reqs = entry.requests;
        map.insert(*pk, entry);
        if let Some(st) = map.get(pk) {
            persist_snapshot(pk, st);
        }
        update_active_gauge(map.len());
        update_memory_usage(map.len());
        #[cfg(feature = "telemetry")]
        {
            if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                let sample = PEER_METRICS_SAMPLE_RATE.load(Ordering::Relaxed);
                if sample <= 1 || reqs % sample == 0 {
                    let id = overlay_peer_label(pk);
                    crate::telemetry::PEER_REQUEST_TOTAL
                        .with_label_values(&[id.as_str()])
                        .inc_by(sample as u64);
                }
            }
        }
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some(_old) = evict_lru(&mut map) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&_old);
                    if crate::telemetry::should_log("p2p") {
                        let id = overlay_peer_label(&_old);
                        diagnostics::tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        entry.requests += 1;
        update_peer_rates(pk, &mut entry, 0, 1, now);
        entry.last_updated = now;
        #[cfg(feature = "telemetry")]
        let reqs = entry.requests;
        map.insert(*pk, entry);
        if let Some(st) = map.get(pk) {
            persist_snapshot(pk, st);
        }
        update_active_gauge(map.len());
        update_memory_usage(map.len());
        #[cfg(feature = "telemetry")]
        {
            if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                let sample = PEER_METRICS_SAMPLE_RATE.load(Ordering::Relaxed);
                if sample <= 1 || reqs % sample == 0 {
                    let id = overlay_peer_label(pk);
                    crate::telemetry::PEER_REQUEST_TOTAL
                        .with_label_values(&[id.as_str()])
                        .inc_by(sample as u64);
                }
            }
        }
    }
}

fn record_drop(pk: &[u8; 32], reason: DropReason) {
    let reason = if TRACK_DROP_REASONS.load(Ordering::Relaxed) {
        reason
    } else {
        DropReason::Other
    };
    let mut map = peer_metrics_guard();
    maybe_consolidate(&mut map);
    if let Some(mut entry) = map.swap_remove(pk) {
        *entry.drops.entry(reason).or_default() += 1;
        if reason == DropReason::TooBusy {
            entry.reputation.penalize(0.9);
            update_reputation_metric(pk, entry.reputation.score);
        }
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        if let Some(st) = map.get(pk) {
            persist_snapshot(pk, st);
        }
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some(_old) = evict_lru(&mut map) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&_old);
                    if crate::telemetry::should_log("p2p") {
                        let id = overlay_peer_label(&_old);
                        diagnostics::tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        *entry.drops.entry(reason).or_default() += 1;
        if reason == DropReason::TooBusy {
            entry.reputation.penalize(0.9);
            update_reputation_metric(pk, entry.reputation.score);
        }
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        if let Some(st) = map.get(pk) {
            persist_snapshot(pk, st);
        }
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = overlay_peer_label(pk);
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), reason.as_ref()])
                .inc();
        }
    }
    #[cfg(feature = "quic")]
    super::quic_stats::record_handshake_failure(pk);
}

fn record_handshake_fail(pk: &[u8; 32], reason: HandshakeError) {
    if !TRACK_HANDSHAKE_FAIL.load(Ordering::Relaxed) {
        return;
    }
    let mut map = peer_metrics_guard();
    maybe_consolidate(&mut map);
    if let Some(mut entry) = map.swap_remove(pk) {
        *entry.handshake_fail.entry(reason).or_default() += 1;
        if matches!(reason, HandshakeError::Tls | HandshakeError::Certificate) {
            entry.tls_errors += 1;
        }
        entry.reputation.penalize(0.95);
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    } else {
        let max = MAX_PEER_METRICS.load(Ordering::Relaxed);
        if map.len() == max {
            if let Some(_old) = evict_lru(&mut map) {
                #[cfg(feature = "telemetry")]
                {
                    remove_peer_metrics(&_old);
                    if crate::telemetry::should_log("p2p") {
                        let id = overlay_peer_label(&_old);
                        diagnostics::tracing::info!(peer = id.as_str(), "evict_peer_metrics");
                    }
                }
            }
        }
        let mut entry = PeerMetrics::default();
        entry.handshake_fail.insert(reason, 1);
        if matches!(reason, HandshakeError::Tls | HandshakeError::Certificate) {
            entry.tls_errors = 1;
        }
        entry.reputation.penalize(0.95);
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = overlay_peer_label(pk);
            crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
                .with_label_values(&[id.as_str(), reason.as_str()])
                .inc();
            crate::telemetry::HANDSHAKE_FAIL_TOTAL
                .with_label_values(&[reason.as_str()])
                .inc();
            if matches!(reason, HandshakeError::Tls | HandshakeError::Certificate) {
                crate::telemetry::PEER_TLS_ERROR_TOTAL
                    .with_label_values(&[id.as_str()])
                    .inc();
            }
        }
    }
    let ts = now_secs();
    let mut log = HANDSHAKE_LOG.guard();
    log.push_back((ts, overlay_peer_label(pk), reason));
    if log.len() > HANDSHAKE_LOG_CAP {
        log.pop_front();
    }
}

fn record_handshake_success(pk: &[u8; 32]) {
    let mut map = peer_metrics_guard();
    maybe_consolidate(&mut map);
    if let Some(mut entry) = map.swap_remove(pk) {
        entry.handshake_success += 1;
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    } else {
        let mut entry = PeerMetrics::default();
        entry.handshake_success = 1;
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
    #[cfg(feature = "telemetry")]
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let id = overlay_peer_label(pk);
        crate::telemetry::PEER_HANDSHAKE_SUCCESS_TOTAL
            .with_label_values(&[id.as_str()])
            .inc();
    }
}

#[cfg(feature = "quic")]
pub(crate) fn record_handshake_latency(pk: &[u8; 32], ms: u64) {
    let mut map = peer_metrics_guard();
    maybe_consolidate(&mut map);
    if let Some(mut entry) = map.swap_remove(pk) {
        entry.last_handshake_ms = ms;
        entry.last_updated = now_secs();
        broadcast_metrics(pk, &entry);
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    } else {
        let mut entry = PeerMetrics::default();
        entry.last_handshake_ms = ms;
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
    #[cfg(feature = "quic")]
    super::quic_stats::record_latency(pk, ms);
}

pub(crate) fn pk_from_addr(addr: &SocketAddr) -> Option<[u8; 32]> {
    ADDR_MAP.guard().get(addr).copied()
}

#[cfg_attr(not(any(test, feature = "integration-tests")), allow(dead_code))]
pub fn inject_addr_mapping_for_tests(addr: SocketAddr, peer: super::OverlayPeerId) {
    let mut buf = [0u8; 32];
    let bytes = super::overlay_peer_to_bytes(&peer);
    buf.copy_from_slice(&bytes);
    ADDR_MAP.guard().insert(addr, buf);
}

#[cfg(feature = "quic")]
pub(crate) fn record_handshake_fail_addr(addr: SocketAddr, reason: HandshakeError) {
    let ts = now_secs();
    {
        let mut last = LAST_HANDSHAKE_ADDR.guard();
        if let Some(prev) = last.get(&addr) {
            if ts.saturating_sub(*prev) < HANDSHAKE_DEBOUNCE_SECS {
                return;
            }
        }
        last.insert(addr, ts);
    }
    if let Some(pk) = ADDR_MAP.guard().get(&addr).copied() {
        record_handshake_fail(&pk, reason);
    }
}

pub fn simulate_handshake_fail(pk: [u8; 32], reason: HandshakeError) {
    record_handshake_fail(&pk, reason);
}

pub fn recent_handshake_failures() -> Vec<(u64, String, HandshakeError)> {
    HANDSHAKE_LOG.guard().iter().cloned().collect()
}

fn update_reputation_metric(pk: &[u8; 32], score: f64) {
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = overlay_peer_label(pk);
            crate::telemetry::PEER_REPUTATION_SCORE
                .with_label_values(&[id.as_str()])
                .set(score);
        }
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = (pk, score);
}

pub fn reset_peer_metrics(pk: &[u8; 32]) -> bool {
    let mut map = peer_metrics_guard();
    if let Some(entry) = map.get_mut(pk) {
        *entry = PeerMetrics::default();
        entry.last_updated = now_secs();
        #[cfg(feature = "telemetry")]
        {
            if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
                remove_peer_metrics(pk);
                update_reputation_metric(pk, 1.0);
                let id = overlay_peer_label(pk);
                crate::telemetry::PEER_STATS_RESET_TOTAL
                    .with_label_values(&[id.as_str()])
                    .inc();
                if crate::telemetry::should_log("p2p") {
                    diagnostics::tracing::info!(peer = id.as_str(), "reset_peer_metrics");
                }
            }
        }
        update_memory_usage(map.len());
        true
    } else {
        false
    }
}

pub fn rotate_peer_key(old: &[u8; 32], new: [u8; 32]) -> bool {
    let mut map = peer_metrics_guard();
    if let Some((_, metrics)) = map.shift_remove_entry(old) {
        map.insert(new, metrics);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
        drop(map);
        let mut addr_map = ADDR_MAP.guard();
        for val in addr_map.values_mut() {
            if *val == *old {
                *val = new;
            }
        }
        drop(addr_map);
        {
            let path = KEY_HISTORY_PATH.guard().clone();
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            let entry = foundation_serialization::json!({
                "old": hex::encode(old),
                "new": hex::encode(new),
                "ts": now_secs(),
            });
            let _ = writeln!(file, "{}", json::to_string_value(&entry));
        }
        let revoke = now_secs() + KEY_GRACE_SECS;
        ROTATED_KEYS.guard().insert(*old, (new, revoke));
        let mut ids = PEER_IDENTITIES.guard();
        if let Some(mut ident) = ids.remove(old) {
            ident.old_key = Some(*old);
            ident.public_key = new;
            ident.rotated_at = Some(now_secs());
            ids.insert(new, ident);
        }
        drop(ids);
        broadcast_key_rotation(old, &new);
        #[cfg(feature = "telemetry")]
        crate::telemetry::KEY_ROTATION_TOTAL.inc();
        true
    } else {
        false
    }
}

pub fn peer_stats(pk: &[u8; 32]) -> Option<PeerMetrics> {
    let res = peer_metrics_guard().get(pk).cloned();
    #[cfg(feature = "telemetry")]
    if res.is_some() && EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let id = overlay_peer_label(pk);
        crate::telemetry::PEER_STATS_QUERY_TOTAL
            .with_label_values(&[id.as_str()])
            .inc();
    }
    res
}

pub fn export_peer_stats(pk: &[u8; 32], name: &str) -> std::io::Result<bool> {
    let res = (|| {
        let rel = Path::new(name);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            #[cfg(feature = "telemetry")]
            log_suspicious(name);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid path",
            ));
        }
        let fname = rel
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "file"))?;
        if !(fname.ends_with(".json") || fname.ends_with(".json.gz")) {
            #[cfg(feature = "telemetry")]
            log_suspicious(fname);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid extension",
            ));
        }
        let dir = METRICS_EXPORT_DIR.guard().clone();
        std::fs::create_dir_all(&dir)?;
        let path = Path::new(&dir).join(rel);
        if path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            #[cfg(feature = "telemetry")]
            log_suspicious(name);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "symlink not allowed",
            ));
        }
        let metrics = {
            let map = peer_metrics_guard();
            map.get(pk).cloned()
        };
        let metrics =
            metrics.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "peer"))?;
        let json = json::to_vec(&metrics).map_err(json_to_io_error)?;
        let tmp_dir = tempfile::tempdir_in(&dir).map_err(sys_to_io_error)?;
        let mut tmp = NamedTempFile::new_in(tmp_dir.path()).map_err(sys_to_io_error)?;
        tmp.as_file().lock_exclusive().map_err(sys_to_io_error)?;
        tmp.write_all(&json)?;
        tmp.flush()?;
        let overwritten = path.exists();
        tmp.persist(&path).map_err(|e| e.error)?;
        tmp_dir.close().map_err(sys_to_io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW)
                .open(&path)?;
        }
        #[cfg(not(unix))]
        {
            if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "symlink not allowed",
                ));
            }
            std::fs::File::open(&path)?;
        }
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

pub fn export_all_peer_stats(
    name: &str,
    min_rep: Option<f64>,
    active_within: Option<u64>,
) -> std::io::Result<bool> {
    let res = (|| {
        let rel = Path::new(name);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            #[cfg(feature = "telemetry")]
            log_suspicious(name);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid path",
            ));
        }
        let fname = rel
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "file"))?;
        let compress = PEER_METRICS_COMPRESS.load(Ordering::Relaxed);
        if compress
            && !(fname.ends_with(".tar.gz")
                || fname.ends_with(".json")
                || fname.ends_with(".json.gz"))
        {
            #[cfg(feature = "telemetry")]
            log_suspicious(fname);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid extension",
            ));
        }
        let base = METRICS_EXPORT_DIR.guard().clone();
        std::fs::create_dir_all(&base)?;
        let quota = PEER_METRICS_EXPORT_QUOTA.load(Ordering::Relaxed);
        let path = Path::new(&base).join(rel);
        if path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            #[cfg(feature = "telemetry")]
            log_suspicious(name);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "symlink not allowed",
            ));
        }

        let keys: Vec<[u8; 32]> = {
            let map = peer_metrics_guard();
            map.keys().cloned().collect()
        };
        let initial_len = keys.len();
        let mut total_bytes = 0u64;

        if compress {
            let tmp_dir = tempfile::tempdir_in(&base).map_err(sys_to_io_error)?;
            let mut tmp = NamedTempFile::new_in(tmp_dir.path()).map_err(sys_to_io_error)?;
            tmp.as_file().lock_exclusive().map_err(sys_to_io_error)?;
            {
                let enc = flate2::write::GzEncoder::new(&mut tmp, flate2::Compression::default());
                let mut tar = Builder::new(enc);
                for pk in &keys {
                    let m = {
                        let map = peer_metrics_guard();
                        match map.get(pk) {
                            Some(v) => v.clone(),
                            None => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "peer list changed",
                                ))
                            }
                        }
                    };
                    if let Some(r) = min_rep {
                        if m.reputation.score < r {
                            continue;
                        }
                    }
                    if let Some(a) = active_within {
                        let now = now_secs();
                        if now.saturating_sub(m.last_updated) > a {
                            continue;
                        }
                    }
                    let id = overlay_peer_label(pk);
                    let data = json::to_vec(&m).map_err(json_to_io_error)?;
                    total_bytes += data.len() as u64;
                    if total_bytes > quota {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "quota exceeded",
                        ));
                    }
                    let mut header = tar::Header::new_gnu();
                    header.set_size(data.len() as u64);
                    header.set_cksum();
                    tar.append_data(&mut header, format!("{id}.json"), data.as_slice())?;
                }
                tar.finish()?;
            }
            if peer_metrics_guard().len() != initial_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "peer list changed",
                ));
            }
            let overwritten = path.exists();
            tmp.persist(&path).map_err(|e| e.error)?;
            tmp_dir.close().map_err(sys_to_io_error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NOFOLLOW)
                    .open(&path)?;
            }
            #[cfg(not(unix))]
            {
                if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "symlink not allowed",
                    ));
                }
                std::fs::File::open(&path)?;
            }
            #[cfg(feature = "telemetry")]
            diagnostics::log::info!(
                "peer_stats_export_all count={} bytes={}",
                initial_len,
                total_bytes
            );
            Ok(overwritten)
        } else {
            let tmp_dir = TempBuilder::new()
                .prefix("export")
                .tempdir_in(&base)
                .map_err(sys_to_io_error)?;
            for pk in &keys {
                let m = {
                    let map = peer_metrics_guard();
                    match map.get(pk) {
                        Some(v) => v.clone(),
                        None => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "peer list changed",
                            ))
                        }
                    }
                };
                if let Some(r) = min_rep {
                    if m.reputation.score < r {
                        continue;
                    }
                }
                if let Some(a) = active_within {
                    let now = now_secs();
                    if now.saturating_sub(m.last_updated) > a {
                        continue;
                    }
                }
                let id = overlay_peer_label(pk);
                let data = json::to_vec(&m).map_err(json_to_io_error)?;
                total_bytes += data.len() as u64;
                if total_bytes > quota {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "quota exceeded",
                    ));
                }
                std::fs::write(tmp_dir.path().join(format!("{id}.json")), &data)?;
            }
            if peer_metrics_guard().len() != initial_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "peer list changed",
                ));
            }
            let overwritten = path.exists();
            if overwritten {
                std::fs::remove_dir_all(&path)?;
            }
            let tmp_path = tmp_dir.keep();
            std::fs::rename(tmp_path, &path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(O_NOFOLLOW)
                    .open(&path)?;
            }
            #[cfg(not(unix))]
            {
                if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "symlink not allowed",
                    ));
                }
                std::fs::File::open(&path)?;
            }
            #[cfg(feature = "telemetry")]
            diagnostics::log::info!(
                "peer_stats_export_all count={} bytes={} ",
                initial_len,
                total_bytes
            );
            Ok(overwritten)
        }
    })();

    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let label = if res.is_ok() { "ok" } else { "error" };
            crate::telemetry::PEER_STATS_EXPORT_ALL_TOTAL
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
    let metrics = PEER_METRICS.guard();
    metrics
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(pk, m)| PeerStat {
            peer_id: overlay_peer_label(pk),
            metrics: m.clone(),
        })
        .collect()
}

pub fn peer_stats_map(
    min_rep: Option<f64>,
    active_within: Option<u64>,
) -> HashMap<String, PeerMetrics> {
    let now = now_secs();
    let metrics = PEER_METRICS.guard();
    metrics
        .iter()
        .filter(|(_, m)| min_rep.map_or(true, |r| m.reputation.score >= r))
        .filter(|(_, m)| active_within.map_or(true, |s| now.saturating_sub(m.last_updated) <= s))
        .map(|(pk, m)| (overlay_peer_label(pk), m.clone()))
        .collect()
}

#[cfg(feature = "telemetry")]
fn remove_peer_metrics(pk: &[u8; 32]) {
    let id = overlay_peer_label(pk);
    let _ = crate::telemetry::PEER_REQUEST_TOTAL.remove_label_values(&[id.as_str()]);
    let _ = crate::telemetry::PEER_BYTES_SENT_TOTAL.remove_label_values(&[id.as_str()]);
    for reason in DROP_REASON_VARIANTS {
        let _ =
            crate::telemetry::PEER_DROP_TOTAL.remove_label_values(&[id.as_str(), reason.as_ref()]);
    }
    for reason in HANDSHAKE_ERROR_VARIANTS {
        let _ = crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
            .remove_label_values(&[id.as_str(), reason.as_str()]);
    }
    let _ = crate::telemetry::PEER_REPUTATION_SCORE.remove_label_values(&[id.as_str()]);
}

#[cfg(not(feature = "telemetry"))]
#[allow(dead_code)]
fn remove_peer_metrics(_pk: &[u8; 32]) {}

#[cfg(test)]
static RECORDED_DROPS: Lazy<Mutex<Vec<SocketAddr>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[cfg(test)]
pub(crate) fn take_recorded_drops() -> Vec<SocketAddr> {
    RECORDED_DROPS.guard().drain(..).collect()
}

pub fn record_ip_drop(ip: &SocketAddr) {
    #[cfg(not(feature = "telemetry"))]
    let _ = ip;
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = ip.to_string();
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), DropReason::Duplicate.as_ref()])
                .inc();
        }
    }
    #[cfg(test)]
    {
        RECORDED_DROPS.guard().push(*ip);
    }
}

fn peer_db_path() -> PathBuf {
    std::env::var("TB_PEER_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpd::{Method, Response, Router, ServerConfig, StatusCode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use sys::tempfile::tempdir;

    struct EnvGuard {
        prev_limit: u32,
        prev_peer: Option<String>,
        prev_quic: Option<String>,
    }

    impl EnvGuard {
        fn new(prev_limit: u32, prev_peer: Option<String>, prev_quic: Option<String>) -> Self {
            Self {
                prev_limit,
                prev_peer,
                prev_quic,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            set_p2p_max_per_sec(self.prev_limit);
            match &self.prev_peer {
                Some(v) => std::env::set_var("TB_PEER_DB_PATH", v),
                None => std::env::remove_var("TB_PEER_DB_PATH"),
            }
            match &self.prev_quic {
                Some(v) => std::env::set_var("TB_QUIC_PEER_DB_PATH", v),
                None => std::env::remove_var("TB_QUIC_PEER_DB_PATH"),
            }
        }
    }

    #[test]
    fn rate_limiting_penalizes_and_bans_peer() {
        let dir = tempdir().expect("temp dir");
        let peers_path = dir.path().join("peers.txt");
        let quic_path = dir.path().join("quic_peers.txt");
        let prev_peer = std::env::var("TB_PEER_DB_PATH").ok();
        let prev_quic = std::env::var("TB_QUIC_PEER_DB_PATH").ok();
        std::env::set_var("TB_PEER_DB_PATH", &peers_path);
        std::env::set_var("TB_QUIC_PEER_DB_PATH", &quic_path);
        ban_store::init(dir.path().join("bans").to_str().expect("path"));

        let previous_limit = p2p_max_per_sec();
        set_p2p_max_per_sec(1);
        let _guard = EnvGuard::new(previous_limit, prev_peer, prev_quic);

        let set = PeerSet::new(Vec::new());
        let pk = [7u8; 32];

        assert!(set.check_rate(&pk).is_ok());
        let err = set
            .check_rate(&pk)
            .expect_err("should rate limit on second request");
        assert!(matches!(err, PeerErrorCode::RateLimit));

        {
            let metrics = peer_metrics_guard();
            let entry = metrics.get(&pk).expect("metrics entry");
            assert!(entry.reputation.score < 1.0);
        }

        let next = set.check_rate(&pk).expect_err("peer should remain banned");
        assert!(matches!(next, PeerErrorCode::Banned));
    }

    #[derive(Clone)]
    struct TestState {
        flag: Arc<AtomicBool>,
    }

    #[test]
    fn aggregator_failover_selects_next_url() {
        runtime::block_on(async {
            let received = Arc::new(AtomicBool::new(false));
            let router = Router::new(TestState {
                flag: received.clone(),
            })
            .route(Method::Post, "/ingest", |req| async move {
                let state = req.state().clone();
                state.flag.store(true, Ordering::SeqCst);
                Ok(Response::new(StatusCode::OK))
            });
            let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let listener = runtime::net::TcpListener::bind(bind_addr).await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = runtime::spawn(async move {
                httpd::serve(listener, router, ServerConfig::default())
                    .await
                    .unwrap();
            });
            let bad = "http://127.0.0.1:59999".to_string();
            let good = format!("http://{}", addr);
            let client = AggregatorClient::new(vec![bad, good], "t".into());
            aggregator_guard().replace(client.clone());
            let snap = PeerSnapshot {
                peer_id: "p".into(),
                metrics: PeerMetrics::default(),
            };
            client.ingest(vec![snap]).await;
            assert!(received.load(Ordering::SeqCst));
            server.abort();
        });
    }
}

fn quic_peer_db_path() -> PathBuf {
    std::env::var("TB_QUIC_PEER_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("quic_peers.txt")
        })
}

fn load_quic_peers() -> HashMap<SocketAddr, QuicEndpoint> {
    use base64_fp::decode_standard;
    let mut map = HashMap::new();
    if let Ok(data) = fs::read_to_string(quic_peer_db_path()) {
        for line in data.lines() {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() == 3 {
                if let (Ok(tcp), Ok(quic)) = (parts[0].parse(), parts[1].parse()) {
                    if let Ok(cert) = decode_standard(parts[2]) {
                        map.insert(tcp, QuicEndpoint { addr: quic, cert });
                    }
                }
            }
        }
    }
    map
}

fn persist_quic_peers(map: &HashMap<SocketAddr, QuicEndpoint>) {
    use base64_fp::encode_standard;
    let path = quic_peer_db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = map
        .iter()
        .map(|(tcp, info)| format!("{tcp},{},{}", info.addr, encode_standard(&info.cert)))
        .collect();
    lines.sort();
    let _ = fs::write(path, lines.join("\n"));
}

fn chunk_db_path() -> PathBuf {
    std::env::var("TB_CHUNK_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
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
    Mutex::new(SimpleDb::open_named(
        names::NET_PEER_CHUNKS,
        path.to_str().unwrap(),
    ))
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

/// Return known peers with transport information and optional QUIC certificates.
pub fn known_peers_with_info() -> Vec<(SocketAddr, Transport, Option<Vec<u8>>)> {
    PeerSet::new(Vec::new()).list_with_info()
}

static P2P_MAX_PER_SEC: Lazy<AtomicU32> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_MAX_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    AtomicU32::new(val)
});

static P2P_MAX_BYTES_PER_SEC: Lazy<AtomicU64> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_MAX_BYTES_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(65536);
    AtomicU64::new(val)
});

static P2P_THROTTLE_SECS: Lazy<u64> = Lazy::new(|| {
    std::env::var("TB_THROTTLE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
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

fn peer_metrics_guard() -> std::sync::MutexGuard<'static, IndexMap<[u8; 32], PeerMetrics>> {
    PEER_METRICS.guard()
}

static ADDR_MAP: Lazy<Mutex<HashMap<SocketAddr, [u8; 32]>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static MAX_PEER_METRICS: AtomicUsize = AtomicUsize::new(1024);
static EXPORT_PEER_METRICS: AtomicBool = AtomicBool::new(true);
#[cfg(feature = "telemetry")]
static SUSPICIOUS_EXPORTS: AtomicU64 = AtomicU64::new(0);
static TRACK_DROP_REASONS: AtomicBool = AtomicBool::new(true);
static TRACK_HANDSHAKE_FAIL: AtomicBool = AtomicBool::new(true);
static PEER_REPUTATION_DECAY: AtomicU64 = AtomicU64::new(f64::to_bits(0.01));
static PEER_METRICS_SAMPLE_RATE: AtomicU64 = AtomicU64::new(1);
static LAST_CONSOLIDATE: AtomicU64 = AtomicU64::new(0);
const CONSOLIDATE_SECS: u64 = 60;
static PEER_METRICS_PATH: Lazy<Mutex<String>> =
    Lazy::new(|| Mutex::new("state/peer_metrics.json".into()));
static METRICS_EXPORT_DIR: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("state".into()));
static PEER_METRICS_EXPORT_QUOTA: AtomicU64 = AtomicU64::new(10 * 1024 * 1024);
static PEER_METRICS_RETENTION: AtomicU64 = AtomicU64::new(7 * 24 * 60 * 60);
static PEER_METRICS_COMPRESS: AtomicBool = AtomicBool::new(false);
static KEY_HISTORY_PATH: Lazy<Mutex<String>> = Lazy::new(|| {
    let path = std::env::var("TB_PEER_KEY_HISTORY_PATH")
        .unwrap_or_else(|_| "state/peer_key_history.log".into());
    Mutex::new(path)
});
/// Mapping of old keys to (new key, revoke timestamp).
static ROTATED_KEYS: Lazy<Mutex<HashMap<[u8; 32], ([u8; 32], u64)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
/// Known peer identities keyed by their current public key.
static PEER_IDENTITIES: Lazy<Mutex<HashMap<[u8; 32], PeerIdentity>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const KEY_GRACE_SECS: u64 = 60 * 5;

/// Recent handshake failures for debug introspection.
static HANDSHAKE_LOG: Lazy<Mutex<VecDeque<(u64, String, HandshakeError)>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
const HANDSHAKE_LOG_CAP: usize = 128;
#[cfg(feature = "quic")]
static LAST_HANDSHAKE_ADDR: Lazy<Mutex<HashMap<SocketAddr, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
#[cfg(feature = "quic")]
const HANDSHAKE_DEBOUNCE_SECS: u64 = 1;
#[allow(dead_code)]
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

#[derive(Clone)]
struct AggregatorClient {
    urls: Vec<String>,
    token: String,
    client: httpd::HttpClient,
    idx: Arc<AtomicUsize>,
    handle: runtime::RuntimeHandle,
}

impl AggregatorClient {
    fn new(urls: Vec<String>, token: String) -> Self {
        Self {
            urls,
            token,
            client: httpd::HttpClient::default(),
            idx: Arc::new(AtomicUsize::new(0)),
            handle: runtime::handle(),
        }
    }

    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let _ = self.handle.spawn(fut);
    }

    async fn ingest(&self, snaps: Vec<PeerSnapshot>) {
        let body = json::to_value(snaps).unwrap();
        self.post("ingest", body).await;
    }

    #[cfg(feature = "telemetry")]
    async fn telemetry_summary(&self, summary: crate::telemetry::summary::TelemetrySummary) {
        let body = json::to_value(summary).unwrap();
        self.post("telemetry", body).await;
    }

    async fn post(&self, path: &str, body: Value) {
        for i in 0..self.urls.len() {
            let idx = (self.idx.load(Ordering::Relaxed) + i) % self.urls.len();
            let url = &self.urls[idx];
            let request = match self
                .client
                .request(httpd::Method::Post, &format!("{}/{}", url, path))
            {
                Ok(builder) => builder.header("x-auth-token", self.token.clone()),
                Err(_) => continue,
            };
            let request = match request.json(&body) {
                Ok(builder) => builder,
                Err(_) => continue,
            };
            match request.send().await {
                Ok(_) => {
                    self.idx.store(idx, Ordering::Relaxed);
                    break;
                }
                Err(_) => continue,
            }
        }
    }
}

static AGGREGATOR: Lazy<Mutex<Option<AggregatorClient>>> = Lazy::new(|| Mutex::new(None));

fn aggregator_guard() -> std::sync::MutexGuard<'static, Option<AggregatorClient>> {
    AGGREGATOR.guard()
}

#[cfg(feature = "telemetry")]
pub fn publish_telemetry_summary(summary: crate::telemetry::summary::TelemetrySummary) {
    if let Some(client) = aggregator_guard().clone() {
        let fut_client = client.clone();
        client.spawn(async move {
            fut_client.telemetry_summary(summary).await;
        });
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn publish_telemetry_summary<T>(_summary: T) {}

fn ban_store_guard() -> std::sync::MutexGuard<'static, ban_store::BanStore> {
    ban_store::store().guard()
}

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
        peer_id: overlay_peer_label(pk),
        metrics: m.clone(),
    };
    if METRIC_TX.send(snap.clone()).is_err() {
        DROPPED_FRAMES.fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "telemetry")]
        crate::telemetry::PEER_METRICS_DROPPED.inc();
    }
    if let Some(client) = {
        let guard = aggregator_guard();
        guard.clone()
    } {
        let fut_client = client.clone();
        client.spawn(async move {
            fut_client.ingest(vec![snap]).await;
        });
    }
}

fn broadcast_key_rotation(old: &[u8; 32], new: &[u8; 32]) {
    if let Some(client) = {
        let guard = aggregator_guard();
        guard.clone()
    } {
        #[derive(Serialize)]
        struct RotationEvent {
            peer_id: String,
            metrics: Value,
        }
        let event = RotationEvent {
            peer_id: overlay_peer_label(old),
            metrics: foundation_serialization::json!({ "key_rotation": hex::encode(new) }),
        };
        let fut_client = client.clone();
        client.spawn(async move {
            let body = json::to_value(vec![event]).unwrap();
            fut_client.post("ingest", body).await;
        });
    }
}

#[allow(dead_code)]
const DROP_REASON_VARIANTS: &[DropReason] = &[
    DropReason::RateLimit,
    DropReason::Malformed,
    DropReason::Blacklist,
    DropReason::Duplicate,
    DropReason::TooBusy,
    DropReason::Other,
];

#[allow(dead_code)]
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

pub fn track_handshake_fail_enabled() -> bool {
    TRACK_HANDSHAKE_FAIL.load(Ordering::Relaxed)
}

pub fn set_peer_reputation_decay(rate: f64) {
    PEER_REPUTATION_DECAY.store(rate.to_bits(), Ordering::Relaxed);
}

pub fn set_metrics_aggregator(cfg: Option<AggregatorConfig>) {
    let mut guard = aggregator_guard();
    if let Some(cfg) = cfg {
        let mut urls = vec![cfg.url];
        if let Some(srv) = cfg.srv_record {
            if let Ok(records) = lookup_srv(&srv) {
                for rec in records {
                    let host = rec.target.trim_end_matches('.');
                    let url = format!("https://{}:{}", host, rec.port);
                    urls.push(url);
                }
            }
        }
        urls.retain(|u| !u.is_empty());
        if urls.is_empty() {
            *guard = None;
        } else {
            *guard = Some(AggregatorClient::new(urls, cfg.auth_token));
        }
    } else {
        *guard = None;
    }
}

pub fn set_peer_metrics_sample_rate(rate: u64) {
    let rate = rate.max(1);
    PEER_METRICS_SAMPLE_RATE.store(rate, Ordering::Relaxed);
}

pub fn set_peer_metrics_path(path: String) {
    *PEER_METRICS_PATH.guard() = path;
}

pub fn set_peer_metrics_retention(ttl: u64) {
    PEER_METRICS_RETENTION.store(ttl, Ordering::Relaxed);
}

pub fn set_metrics_export_dir(dir: String) {
    *METRICS_EXPORT_DIR.guard() = dir;
}

pub fn set_peer_metrics_compress(val: bool) {
    PEER_METRICS_COMPRESS.store(val, Ordering::Relaxed);
}

pub fn set_peer_metrics_export_quota(bytes: u64) {
    PEER_METRICS_EXPORT_QUOTA.store(bytes, Ordering::Relaxed);
}

pub fn set_p2p_max_per_sec(v: u32) {
    P2P_MAX_PER_SEC.store(v, Ordering::Relaxed);
}

pub fn p2p_max_per_sec() -> u32 {
    P2P_MAX_PER_SEC.load(Ordering::Relaxed)
}

pub fn set_p2p_max_bytes_per_sec(v: u64) {
    P2P_MAX_BYTES_PER_SEC.store(v, Ordering::Relaxed);
}

pub fn p2p_max_bytes_per_sec() -> u64 {
    P2P_MAX_BYTES_PER_SEC.load(Ordering::Relaxed)
}

pub fn throttle_peer(pk: &[u8; 32], reason: &str) {
    let mut map = peer_metrics_guard();
    let entry = map.entry(*pk).or_insert_with(PeerMetrics::default);
    let now = now_secs();
    let dur = *P2P_THROTTLE_SECS << entry.backoff_level.min(5);
    entry.throttled_until = now + dur;
    entry.throttle_reason = Some(reason.to_string());
    entry.breach_count = 0;
    entry.backoff_level = entry.backoff_level.saturating_add(1);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::PEER_THROTTLE_TOTAL
            .with_label_values(&[reason])
            .inc();
        crate::telemetry::PEER_BACKPRESSURE_ACTIVE_TOTAL
            .with_label_values(&[reason])
            .inc();
        if crate::telemetry::should_log("p2p") {
            let id = overlay_peer_label(pk);
            diagnostics::tracing::warn!(
                peer = id.as_str(),
                reason,
                duration = dur,
                "peer_throttled"
            );
        }
    }
}

pub fn clear_throttle(pk: &[u8; 32]) -> bool {
    let mut map = peer_metrics_guard();
    if let Some(e) = map.get_mut(pk) {
        e.throttled_until = 0;
        e.throttle_reason = None;
        e.breach_count = 0;
        e.backoff_level = 0;
        true
    } else {
        false
    }
}

fn is_throttled(pk: &[u8; 32]) -> bool {
    let map = peer_metrics_guard();
    map.get(pk)
        .map(|e| e.throttled_until > now_secs())
        .unwrap_or(false)
}

pub(crate) fn is_throttled_addr(addr: &SocketAddr) -> bool {
    if let Some(pk) = ADDR_MAP.guard().get(addr) {
        is_throttled(pk)
    } else {
        false
    }
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
fn update_memory_usage(len: usize) {
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let bytes = len * size_of::<PeerMetrics>();
        crate::telemetry::PEER_METRICS_MEM_BYTES.set(bytes as i64);
    }
}

#[cfg(not(feature = "telemetry"))]
fn update_memory_usage(_len: usize) {}

fn maybe_consolidate(map: &mut IndexMap<[u8; 32], PeerMetrics>) {
    let now = now_secs();
    let last = LAST_CONSOLIDATE.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= CONSOLIDATE_SECS
        && LAST_CONSOLIDATE
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        let mut removed = Vec::new();
        map.retain(|pk, m| {
            let keep = m.requests > 0
                || m.bytes_sent > 0
                || m.drops.values().any(|&v| v > 0)
                || m.handshake_fail.values().any(|&v| v > 0);
            if !keep {
                removed.push(*pk);
            }
            keep
        });
        update_active_gauge(map.len());
        update_memory_usage(map.len());
        #[cfg(feature = "telemetry")]
        for pk in removed {
            remove_peer_metrics(&pk);
        }
    }
}

fn evict_lru(map: &mut IndexMap<[u8; 32], PeerMetrics>) -> Option<[u8; 32]> {
    let now = now_secs();
    if let Some(pos) = map
        .iter()
        .position(|(_, m)| m.throttled_until == 0 || m.throttled_until <= now)
    {
        map.swap_remove_index(pos).map(|(k, _)| k)
    } else {
        map.swap_remove_index(0).map(|(k, _)| k)
    }
}

#[cfg(feature = "telemetry")]
fn register_peer_metrics(pk: &[u8; 32], m: &PeerMetrics) {
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        let id = overlay_peer_label(pk);
        crate::telemetry::PEER_REQUEST_TOTAL
            .with_label_values(&[id.as_str()])
            .inc_by(m.requests);
        crate::telemetry::PEER_BYTES_SENT_TOTAL
            .with_label_values(&[id.as_str()])
            .inc_by(m.bytes_sent);
        for (r, c) in &m.drops {
            crate::telemetry::PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), r.as_ref()])
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

fn persist_snapshot(pk: &[u8; 32], m: &PeerMetrics) {
    if let Some(store) = peer_metrics_store::store() {
        let ttl = PEER_METRICS_RETENTION.load(Ordering::Relaxed);
        store.insert(pk, m, ttl);
    }
}

pub fn persist_peer_metrics() -> std::io::Result<()> {
    if let Some(store) = peer_metrics_store::store() {
        store
            .flush()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }
    Ok(())
}

pub fn load_peer_metrics() {
    let ttl = PEER_METRICS_RETENTION.load(Ordering::Relaxed);
    if let Some(store) = peer_metrics_store::store() {
        let entries = store.load(ttl);
        let mut map = peer_metrics_guard();
        #[cfg(feature = "telemetry")]
        let export = EXPORT_PEER_METRICS.load(Ordering::Relaxed);
        #[cfg(not(feature = "telemetry"))]
        let export = false;
        map.clear();
        for (pk, m) in entries {
            if export {
                register_peer_metrics(&pk, &m);
            }
            map.insert(pk, m);
        }
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
}

pub fn clear_peer_metrics() {
    let mut map = peer_metrics_guard();
    #[cfg(feature = "telemetry")]
    if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
        for pk in map.keys().cloned().collect::<Vec<_>>() {
            remove_peer_metrics(&pk);
        }
    }
    map.clear();
    update_active_gauge(0);
    update_memory_usage(0);
}
