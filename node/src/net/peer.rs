use super::{
    load_net_key, overlay_peer_from_bytes, overlay_peer_to_base58, send_msg, LOCAL_FEATURES,
    PROTOCOL_VERSION,
};
use crate::config::AggregatorConfig;
#[cfg(feature = "telemetry")]
use crate::consensus::observer;
use crate::http_client;
use crate::net::message::{encode_payload, ChainRequest, Message, Payload};
use crate::net::Bytes;
#[cfg(feature = "quic")]
use crate::p2p::handshake::validate_quic_certificate;
use crate::p2p::handshake::{Hello, Transport};
use crate::simple_db::{names, SimpleDb};
use crate::storage::provider_directory;
use crate::{Block, Blockchain};
use concurrency::{Lazy, MutexExt, OrderedMap};
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use foundation_serialization::{
    json::{self, Map, Value},
    Error as SerializationError,
};
use foundation_serialization::{Deserialize, Serialize};
use rand::{rngs::OsRng, rngs::StdRng, seq::SliceRandom, RngCore};
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
#[cfg(feature = "integration-tests")]
use std::sync::OnceLock;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use sys::fs::{FileLockExt, O_NOFOLLOW};

use sys::paths;
use sys::tempfile::{Builder as TempBuilder, NamedTempFile};

use foundation_archive::{gzip, tar};

fn sys_to_io_error(err: sys::error::SysError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

fn json_to_io_error(err: SerializationError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

fn local_hello_for_handshake(local_addr: Option<SocketAddr>) -> Hello {
    let agent = format!("blockd/{}", env!("CARGO_PKG_VERSION"));
    let mut nonce_bytes = [0u8; 8];
    OsRng::default().fill_bytes(&mut nonce_bytes);
    let nonce = u64::from_le_bytes(nonce_bytes);
    Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: LOCAL_FEATURES,
        agent,
        nonce,
        transport: Transport::Tcp,
        gossip_addr: local_addr,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        quic_provider: None,
        quic_capabilities: Vec::new(),
    }
}

use super::{
    ban_store::{self, BanStoreError},
    peer_metrics_store,
};
#[cfg(feature = "quic")]
use super::{record_peer_certificate, verify_peer_fingerprint};

#[cfg(feature = "telemetry")]
fn telemetry_handle<T, F>(metric: &'static str, labels: &[&str], fetch: F) -> Option<T>
where
    F: FnOnce() -> runtime::telemetry::Result<T>,
{
    match fetch() {
        Ok(handle) => Some(handle),
        Err(err) => {
            let label_snapshot: Vec<String> =
                labels.iter().map(|label| (*label).to_string()).collect();
            diagnostics::tracing::warn!(
                target: "telemetry",
                %metric,
                labels = ?label_snapshot,
                %err,
                "failed to obtain telemetry handle"
            );
            None
        }
    }
}

#[cfg(feature = "telemetry")]
fn with_metric_handle<T, Fetch, Action>(
    metric: &'static str,
    labels: &[&str],
    fetch: Fetch,
    action: Action,
) where
    Fetch: FnOnce() -> runtime::telemetry::Result<T>,
    Action: FnOnce(T),
{
    if let Some(handle) = telemetry_handle(metric, labels, fetch) {
        action(handle);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

const P2P_MIN_CHAIN_REBROADCAST_MS: u64 = 25;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(feature = "telemetry")]
fn log_suspicious(path: &str) {
    let count = SUSPICIOUS_EXPORTS.fetch_add(1, Ordering::Relaxed) + 1;
    if count % 100 == 0 {
        diagnostics::tracing::warn!(%path, "suspicious metrics export attempt count={}", count);
    }
}

fn overlay_peer_label(pk: &[u8; 32]) -> String {
    overlay_peer_from_bytes(pk)
        .map(|peer| overlay_peer_to_base58(&peer))
        .unwrap_or_else(|_| crypto_suite::hex::encode(pk))
}

fn addr_from_peer_key(peer_key: &[u8; 32]) -> Option<SocketAddr> {
    ADDR_MAP
        .guard()
        .iter()
        .find_map(|(addr, key)| (key == peer_key).then_some(*addr))
}

fn send_chain_snapshot(peers: &PeerSet, addr: Option<SocketAddr>, chain: Vec<Block>) {
    if chain.is_empty() {
        return;
    }
    let msg = match Message::new_with_cert_fingerprint(
        Payload::Chain(chain),
        &peers.key,
        peers.cert_fingerprint(),
    ) {
        Ok(msg) => msg,
        Err(err) => {
            diagnostics::tracing::error!(
                target = "net",
                reason = %err,
                "failed_to_sign_chain_payload"
            );
            return;
        }
    };
    let msg = Arc::new(msg);
    let peers_snapshot = peers.list();
    if let Some(peer_addr) = addr {
        send_msg_with_backoff(peer_addr, Arc::clone(&msg), 3);
        return;
    }
    for peer in peers_snapshot {
        send_msg_with_backoff(peer, Arc::clone(&msg), 3);
    }
}

fn send_chain_request(peers: &PeerSet, addr: Option<SocketAddr>, from_height: u64) {
    let request = ChainRequest { from_height };
    let msg = match Message::new_with_cert_fingerprint(
        Payload::ChainRequest(request),
        &peers.key,
        peers.cert_fingerprint(),
    ) {
        Ok(msg) => msg,
        Err(err) => {
            diagnostics::tracing::error!(
                target = "net",
                reason = %err,
                "failed_to_sign_chain_request"
            );
            return;
        }
    };
    let msg = Arc::new(msg);
    let peers_snapshot = peers.list();
    if let Some(peer_addr) = addr {
        if peers_snapshot.contains(&peer_addr) {
            send_msg_with_backoff(peer_addr, Arc::clone(&msg), 3);
            return;
        }
    }
    for peer in peers_snapshot {
        send_msg_with_backoff(peer, Arc::clone(&msg), 3);
    }
}

fn send_msg_with_backoff(addr: SocketAddr, msg: Arc<Message>, attempts: usize) {
    if attempts == 0 {
        return;
    }
    std::thread::spawn(move || {
        let mut delay = Duration::from_millis(50);
        for attempt in 0..attempts {
            match send_msg(addr, &msg) {
                Ok(()) => return,
                Err(_e) => {}
            }
            std::thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_millis(400));
        }
    });
}

fn validate_metrics_archive(path: &Path) -> std::io::Result<()> {
    let file = fs::File::open(path)?;
    let decoder = gzip::Decoder::new(file)?;
    let mut reader = tar::Reader::new(decoder);
    while let Some(entry) = reader.next()? {
        if entry.size() as usize != entry.data().len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive entry truncated",
            ));
        }
    }
    Ok(())
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
#[derive(Clone)]
pub struct PeerSet {
    addrs: Arc<Mutex<HashSet<SocketAddr>>>,
    authorized: Arc<Mutex<HashSet<[u8; 32]>>>,
    states: Arc<Mutex<HashMap<[u8; 32], PeerState>>>,
    transports: Arc<Mutex<HashMap<SocketAddr, Transport>>>,
    quic: Arc<Mutex<HashMap<SocketAddr, QuicEndpoint>>>,
    peer_db_path: PathBuf,
    quic_peer_db_path: PathBuf,
    key: SigningKey,
    local_addr: Option<SocketAddr>,
    cert_fingerprint: Arc<Mutex<Option<Bytes>>>,
    broadcast_pending: Arc<Mutex<Option<Vec<Block>>>>,
    broadcast_active: Arc<AtomicBool>,
    /// Track the length of the last chain we successfully broadcast to avoid redundant broadcasts.
    last_broadcast_len: Arc<AtomicUsize>,
    /// Timestamp (millis since UNIX_EPOCH) of the last chain broadcast attempt.
    last_broadcast_ms: Arc<AtomicU64>,
}

impl PeerSet {
    /// Create a new set seeded with `initial` peers and any persisted peers.
    pub fn new(initial: Vec<SocketAddr>) -> Self {
        let key = load_net_key();
        Self::new_with_key(initial, key)
    }

    /// Create a new set seeded with `initial` peers using the provided signing key.
    pub fn new_with_key(initial: Vec<SocketAddr>, key: SigningKey) -> Self {
        Self::new_with_key_and_addr(initial, key, None)
    }

    /// Create a new set with an explicit local gossip address (used for reply routing).
    pub fn new_with_key_and_addr(
        initial: Vec<SocketAddr>,
        key: SigningKey,
        local_addr: Option<SocketAddr>,
    ) -> Self {
        let peer_db_path = peer_db_path_from_env();
        let mut set: HashSet<_> = initial.into_iter().collect();
        if let Ok(data) = fs::read_to_string(&peer_db_path) {
            for line in data.lines() {
                if let Ok(addr) = line.trim().parse::<SocketAddr>() {
                    set.insert(addr);
                }
            }
        }
        persist_peers(&peer_db_path, &set);
        let quic_peer_db_path = quic_peer_db_path_from_env();
        let quic_map = load_quic_peers(&quic_peer_db_path);
        Self {
            addrs: Arc::new(Mutex::new(set)),
            authorized: Arc::new(Mutex::new(HashSet::new())),
            states: Arc::new(Mutex::new(HashMap::new())),
            transports: Arc::new(Mutex::new(HashMap::new())),
            quic: Arc::new(Mutex::new(quic_map)),
            peer_db_path,
            quic_peer_db_path,
            key,
            local_addr,
            cert_fingerprint: Arc::new(Mutex::new(None)),
            broadcast_pending: Arc::new(Mutex::new(None)),
            broadcast_active: Arc::new(AtomicBool::new(false)),
            last_broadcast_len: Arc::new(AtomicUsize::new(0)),
            last_broadcast_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_cert_fingerprint(&self, cert_fingerprint: Option<Bytes>) {
        *self.cert_fingerprint.guard() = cert_fingerprint;
    }

    /// Clear the broadcast watermark so the next chain broadcast is sent even if the height
    /// did not increase. This is used when new peers join so they receive the current tip.
    pub(crate) fn reset_broadcast_watermark(&self) {
        self.last_broadcast_len.store(0, Ordering::Release);
        self.last_broadcast_ms.store(0, Ordering::Release);
    }

    fn cert_fingerprint(&self) -> Option<Bytes> {
        self.cert_fingerprint.guard().clone()
    }

    pub(crate) fn schedule_chain_broadcast(&self, chain: Vec<Block>) {
        if chain.is_empty() {
            return;
        }
        // Liveness: even if length didn't increase, rebroadcast periodically so
        // packet loss can't strand peers forever.
        let last_len = self.last_broadcast_len.load(Ordering::Acquire);
        let last_ms = self.last_broadcast_ms.load(Ordering::Acquire);
        let now_ms = now_millis();

        let len_increased = chain.len() > last_len;
        let time_ok = now_ms.saturating_sub(last_ms) >= P2P_MIN_CHAIN_REBROADCAST_MS;

        if !len_increased && !time_ok {
            return;
        }
        let should_spawn = {
            let mut pending = self.broadcast_pending.guard();
            let replace = match pending.as_ref() {
                None => true,
                Some(existing) => {
                    if chain.len() > existing.len() {
                        true
                    } else if chain.len() == existing.len() {
                        let existing_tip = existing.last().map(|b| &b.hash);
                        let new_tip = chain.last().map(|b| &b.hash);
                        existing_tip != new_tip
                    } else {
                        false
                    }
                }
            };
            if !replace {
                return;
            }
            *pending = Some(chain);
            !self.broadcast_active.swap(true, Ordering::AcqRel)
        };
        if !should_spawn {
            return;
        }
        let peers = self.clone();
        std::thread::spawn(move || {
            let mut backoff = Duration::from_millis(0);
            loop {
                let chain_to_send = {
                    let mut pending = peers.broadcast_pending.guard();
                    pending.take()
                };
                if let Some(chain) = chain_to_send {
                    let chain_len = chain.len();
                    let sent_ms = now_millis();
                    send_chain_snapshot(&peers, None, chain);
                    // Mark "attempted broadcast". (No ACK exists; this is best-effort.)
                    peers.last_broadcast_ms.store(sent_ms, Ordering::Release);
                    peers
                        .last_broadcast_len
                        .fetch_max(chain_len, Ordering::Release);
                }
                if peers.broadcast_pending.guard().is_none() {
                    peers.broadcast_active.store(false, Ordering::Release);
                    if peers.broadcast_pending.guard().is_none() {
                        return;
                    }
                    if peers.broadcast_active.swap(true, Ordering::AcqRel) {
                        return;
                    }
                }
                backoff = if backoff.is_zero() {
                    Duration::from_millis(25)
                } else {
                    (backoff * 2).min(Duration::from_millis(200))
                };
                std::thread::sleep(backoff);
            }
        });
    }

    pub(crate) fn broadcast_chain_snapshot(&self, chain: Vec<Block>) {
        // Delegate to schedule_chain_broadcast to avoid code duplication.
        // The coalescing logic in schedule_chain_broadcast still sends the chain,
        // but may batch it with other pending broadcasts for efficiency.
        self.schedule_chain_broadcast(chain);
    }

    pub(crate) fn request_chain_from(&self, addr: SocketAddr, from_height: u64) {
        send_chain_request(self, Some(addr), from_height);
    }

    /// Add a peer to the set.
    pub fn add(&self, addr: SocketAddr) {
        if Some(addr) == self.local_addr {
            return;
        }
        let mut guard = self.addrs.guard();
        guard.insert(addr);
        persist_peers(&self.peer_db_path, &guard);
        let mut map = self.transports.guard();
        map.entry(addr).or_insert(Transport::Tcp);
        let q = self.quic.guard();
        if !q.contains_key(&addr) {
            persist_quic_peers(&self.quic_peer_db_path, &q);
        }
        // Force tip rebroadcast so new peer gets the current chain
        self.reset_broadcast_watermark();
    }

    /// Remove a peer from the set.
    pub fn remove(&self, addr: SocketAddr) {
        let mut guard = self.addrs.guard();
        guard.remove(&addr);
        persist_peers(&self.peer_db_path, &guard);
        let mut map = self.transports.guard();
        map.remove(&addr);
        // Force tip rebroadcast after topology change
        self.reset_broadcast_watermark();
    }

    /// Clear all peers from the set.
    pub fn clear(&self) {
        let mut guard = self.addrs.guard();
        guard.clear();
        persist_peers(&self.peer_db_path, &guard);
        let mut map = self.transports.guard();
        map.clear();
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.addrs.guard().iter().copied().collect()
    }

    /// Whether we have already mapped this address to a peer identity.
    pub fn is_mapped(&self, addr: SocketAddr) -> bool {
        ADDR_MAP.guard().contains_key(&addr)
    }

    /// Snapshot peers with their advertised transport.
    pub fn list_with_transport(&self) -> Vec<(SocketAddr, Transport)> {
        self.list_with_info()
            .into_iter()
            .map(|(a, t, _)| (a, t))
            .collect()
    }

    /// Snapshot peers with transport and optional QUIC certificate.
    pub fn list_with_info(&self) -> Vec<(SocketAddr, Transport, Option<Bytes>)> {
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
    pub fn set_quic(&self, addr: SocketAddr, quic_addr: SocketAddr, cert: Bytes) {
        let mut map = self.quic.guard();
        map.insert(
            addr,
            QuicEndpoint {
                addr: quic_addr,
                cert,
            },
        );
        persist_quic_peers(&self.quic_peer_db_path, &map);
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
        match ban_store_guard().is_banned(pk) {
            Ok(true) => return Err(PeerErrorCode::Banned),
            Ok(false) => {}
            Err(err) => log_ban_store_error("is_banned", pk, &err),
        }
        let mut map = self.states.guard();
        let entry = map.entry(*pk).or_insert(PeerState {
            count: 0,
            rate_window_start: Instant::now(),
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
        if entry
            .rate_window_start
            .elapsed()
            .as_secs()
            .ge(&P2P_RATE_WINDOW_SECS.load(Ordering::Relaxed))
        {
            entry.rate_window_start = Instant::now();
            entry.count = 0;
        }
        entry.count += 1;
        let limit = p2p_max_per_sec();
        let allowed = {
            let mut metrics = peer_metrics_guard();
            let pm = metrics.entry(*pk).or_insert_with(PeerMetrics::default);
            let network_health = crate::net::health::global_health_tracker()
                .lock()
                .unwrap()
                .current_health_index();
            pm.reputation.decay(peer_reputation_decay(), network_health);
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
            let ts = now_secs() + *P2P_BAN_SECS as u64;
            if let Err(err) = ban_store_guard().ban(pk, ts) {
                log_ban_store_error("ban", pk, &err);
            }
            #[cfg(feature = "telemetry")]
            {
                let id = overlay_peer_label(pk);
                let label = id.as_str();
                let labels = [label];
                with_metric_handle(
                    "p2p_request_limit_hits_total",
                    &labels,
                    || {
                        crate::telemetry::P2P_REQUEST_LIMIT_HITS_TOTAL
                            .ensure_handle_for_label_values(&labels)
                    },
                    |counter| counter.inc(),
                );
            }
            return Err(PeerErrorCode::RateLimit);
        }
        Ok(())
    }

    fn check_shard_rate(&self, pk: &[u8; 32], size: usize) -> Result<(), PeerErrorCode> {
        let mut map = self.states.guard();
        let entry = map.entry(*pk).or_insert(PeerState {
            count: 0,
            rate_window_start: Instant::now(),
            banned_until: None,
            shard_tokens: *P2P_SHARD_BURST as f64,
            shard_last: Instant::now(),
        });
        let score = {
            let mut metrics = peer_metrics_guard();
            let pm = metrics.entry(*pk).or_insert_with(PeerMetrics::default);
            let network_health = crate::net::health::global_health_tracker()
                .lock()
                .unwrap()
                .current_health_index();
            pm.reputation.decay(peer_reputation_decay(), network_health);
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
        let ts = now_secs() + *P2P_BAN_SECS as u64;
        if let Err(err) = ban_store_guard().ban(pk, ts) {
            log_ban_store_error("ban", pk, &err);
        }
        #[cfg(feature = "telemetry")]
        {
            let id = overlay_peer_label(pk);
            let label = id.as_str();
            let labels = [label];
            with_metric_handle(
                "p2p_request_limit_hits_total",
                &labels,
                || {
                    crate::telemetry::P2P_REQUEST_LIMIT_HITS_TOTAL
                        .ensure_handle_for_label_values(&labels)
                },
                |counter| counter.inc(),
            );
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
        let bytes = match encode_payload(&msg.body) {
            Ok(b) => b,
            Err(err) => {
                return;
            }
        };
        let pk = match VerifyingKey::from_bytes(&msg.pubkey) {
            Ok(p) => p,
            Err(_) => {
                return;
            }
        };
        let sig_bytes: [u8; 64] = match msg.signature.as_ref().try_into() {
            Ok(bytes) => bytes,
            Err(_) => {
                return;
            }
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
                arr.copy_from_slice(fp.as_ref());
                Some(arr)
            } else {
                None
            }
        });
        #[cfg(feature = "quic")]
        if !matches!(msg.body, Payload::Handshake(_)) {
            let fingerprint = msg_fingerprint.as_ref().map(|fp| fp);
            let quic_addr = addr
                .as_ref()
                .map(|addr| self.quic.guard().contains_key(addr))
                .unwrap_or(false);
            // Only enforce certificate fingerprints when the message includes one
            // or the peer is known to be using QUIC for this address.
            if (fingerprint.is_some() || quic_addr)
                && !verify_peer_fingerprint(&peer_key, fingerprint)
            {
                record_drop(&peer_key, DropReason::Malformed);
                return;
            }
        }

        record_request(&peer_key);

        if let Err(code) = self.check_rate(&peer_key) {
            telemetry_peer_error(code);
            let reason = match code {
                PeerErrorCode::RateLimit => DropReason::RateLimit,
                PeerErrorCode::Banned => DropReason::Blacklist,
                _ => DropReason::Malformed,
            };
            record_drop(&peer_key, reason);
            if matches!(code, PeerErrorCode::Banned) {
                if let Some(peer_addr) = addr {
                    let mut a = self.addrs.guard();
                    a.remove(&peer_addr);
                }
                self.authorized.guard().remove(&peer_key);
            }
            return;
        }

        // Drop duplicate Hello/Handshake from the same peer address or key with a short TTL
        // to prevent ping-pong connection storms during integration tests.
        if matches!(msg.body, Payload::Hello(_) | Payload::Handshake(_)) {
            if let Some(addr) = addr {
                let dup = {
                    let map = ADDR_MAP.guard();
                    map.get(&addr).map(|pk| pk == &peer_key).unwrap_or(false)
                };
                if dup {
                    return;
                }
            }
        }

        if is_throttled(&peer_key) {
            if let Some(_m) = peer_stats(&peer_key) {
                #[cfg(feature = "telemetry")]
                if let Some(reason) = _m.throttle_reason.as_deref() {
                    let labels = [reason];
                    with_metric_handle(
                        "peer_backpressure_dropped_total",
                        &labels,
                        || {
                            crate::telemetry::PEER_BACKPRESSURE_DROPPED_TOTAL
                                .ensure_handle_for_label_values(&labels)
                        },
                        |counter| counter.inc(),
                    );
                }
            }
            record_drop(&peer_key, DropReason::TooBusy);
            return;
        }

        match msg.body {
            Payload::Handshake(hs) => {
                let was_authorized = self.is_authorized(&peer_key);
                if hs.proto_version != PROTOCOL_VERSION {
                    telemetry_peer_error(PeerErrorCode::HandshakeVersion);
                    #[cfg(feature = "telemetry")]
                    {
                        let labels = ["protocol"];
                        with_metric_handle(
                            "peer_rejected_total",
                            &labels,
                            || {
                                crate::telemetry::PEER_REJECTED_TOTAL
                                    .ensure_handle_for_label_values(&labels)
                            },
                            |counter| counter.inc(),
                        );
                        with_metric_handle(
                            "handshake_fail_total",
                            &labels,
                            || {
                                crate::telemetry::HANDSHAKE_FAIL_TOTAL
                                    .ensure_handle_for_label_values(&labels)
                            },
                            |counter| counter.inc(),
                        );
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
                        let labels = ["feature"];
                        with_metric_handle(
                            "handshake_fail_total",
                            &labels,
                            || {
                                crate::telemetry::HANDSHAKE_FAIL_TOTAL
                                    .ensure_handle_for_label_values(&labels)
                            },
                            |counter| counter.inc(),
                        );
                    }
                    record_handshake_fail(&peer_key, HandshakeError::Other);
                    return;
                }
                if hs.transport != Transport::Tcp && hs.transport != Transport::Quic {
                    telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                    return;
                }
                #[cfg(feature = "quic")]
                let mut validated_cert = None;
                #[cfg(feature = "quic")]
                let mut quic_ok = true;
                #[cfg(feature = "quic")]
                let mut quic_error = None;
                #[cfg(feature = "quic")]
                {
                    let needs_quic_cert = hs.transport == Transport::Quic || hs.quic_addr.is_some();
                    if hs.quic_cert.is_some() {
                        match validate_quic_certificate(&peer_key, &hs) {
                            Ok(v) => validated_cert = v,
                            Err(_) => {
                                telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                                #[cfg(feature = "telemetry")]
                                {
                                    let labels = ["certificate"];
                                    with_metric_handle(
                                        "handshake_fail_total",
                                        &labels,
                                        || {
                                            crate::telemetry::HANDSHAKE_FAIL_TOTAL
                                                .ensure_handle_for_label_values(&labels)
                                        },
                                        |counter| counter.inc(),
                                    );
                                }
                                quic_ok = false;
                                quic_error = Some(HandshakeError::Certificate);
                            }
                        }
                    } else if needs_quic_cert {
                        telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                        #[cfg(feature = "telemetry")]
                        {
                            let labels = ["certificate"];
                            with_metric_handle(
                                "handshake_fail_total",
                                &labels,
                                || {
                                    crate::telemetry::HANDSHAKE_FAIL_TOTAL
                                        .ensure_handle_for_label_values(&labels)
                                },
                                |counter| counter.inc(),
                            );
                        }
                        quic_ok = false;
                        quic_error = Some(HandshakeError::Certificate);
                    }

                    if let Some(reason) = quic_error {
                        if hs.transport == Transport::Quic {
                            record_handshake_fail(&peer_key, reason);
                            return;
                        }
                        record_handshake_issue(&peer_key, reason, false);
                    }
                }
                self.authorize(peer_key);
                record_handshake_success(&peer_key);
                let peer_addr = hs.gossip_addr.or(addr);
                if let Some(peer_addr) = peer_addr {
                    self.add(peer_addr);
                    self.map_addr(peer_addr, peer_key);
                    let transport = {
                        #[cfg(feature = "quic")]
                        {
                            let mut transport = hs.transport;
                            if !quic_ok {
                                transport = Transport::Tcp;
                                diagnostics::tracing::warn!(
                                    target: "p2p",
                                    peer = %overlay_peer_label(&peer_key),
                                    "quic certificate rejected; continuing with tcp only"
                                );
                            }
                            transport
                        }
                        #[cfg(not(feature = "quic"))]
                        {
                            hs.transport
                        }
                    };
                    self.set_transport(peer_addr, transport);
                    #[cfg(feature = "quic")]
                    if quic_ok {
                        if let (Some(qaddr), Some(cert)) = (hs.quic_addr, hs.quic_cert.clone()) {
                            self.set_quic(peer_addr, qaddr, cert.clone());
                        }
                        if let (Some(cert), Some(vc)) =
                            (hs.quic_cert.clone(), validated_cert.as_ref())
                        {
                            record_peer_certificate(
                                &peer_key,
                                &vc.provider,
                                cert,
                                vc.fingerprint,
                                vc.previous.clone(),
                            );
                        }
                    }
                    if !was_authorized {
                        let key = self.key.clone();
                        let cert_fingerprint = self.cert_fingerprint();
                        if let Ok(msg) = Message::new_with_cert_fingerprint(
                            Payload::Handshake(local_hello_for_handshake(self.local_addr)),
                            &key,
                            cert_fingerprint.clone(),
                        ) {
                            let _ = send_msg(peer_addr, &msg);
                        }
                        let chain_snapshot = {
                            let bc = chain.guard();
                            bc.chain.clone()
                        };
                        // On initial authorization, seed the peer with our current view of the
                        // chain and let the regular reconciliation flow request follow-ups only
                        // if we're behind. Avoid immediately issuing a redundant ChainRequest
                        // that creates extra inbound traffic for every new peer.
                        send_chain_snapshot(self, Some(peer_addr), chain_snapshot.clone());
                    }
                }
            }
            Payload::Hello(addrs) => {
                let mut handshake_targets = Vec::new();
                let mut new_peers = Vec::new();
                for a in addrs {
                    if Some(a) == self.local_addr {
                        continue;
                    }
                    let is_new = {
                        let guard = self.addrs.guard();
                        !guard.contains(&a)
                    };
                    self.add(a);
                    if is_new {
                        new_peers.push(a);
                    }
                    if pk_from_addr(&a).is_none() {
                        handshake_targets.push(a);
                    }
                }
                if !handshake_targets.is_empty() {
                    if let Ok(msg) = Message::new_with_cert_fingerprint(
                        Payload::Handshake(local_hello_for_handshake(self.local_addr)),
                        &self.key,
                        self.cert_fingerprint(),
                    ) {
                        for addr in handshake_targets {
                            let _ = send_msg(addr, &msg);
                        }
                    }
                }
                if !new_peers.is_empty() {
                    let chain_snapshot = {
                        let bc = chain.guard();
                        bc.chain.clone()
                    };
                    if !chain_snapshot.is_empty() {
                        if let Ok(msg) = Message::new_with_cert_fingerprint(
                            Payload::Chain(chain_snapshot),
                            &self.key,
                            self.cert_fingerprint(),
                        ) {
                            for addr in new_peers {
                                let _ = send_msg(addr, &msg);
                            }
                        }
                    }
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
                loop {
                    let (chain_snapshot, current_len, params_snapshot, expected_prev) = {
                        let bc = chain.guard();
                        (
                            bc.chain.clone(),
                            bc.chain.len(),
                            bc.params.clone(),
                            bc.chain.last().map(|b| b.hash.clone()),
                        )
                    };
                    if block.index as usize != current_len {
                        return;
                    }
                    if block.index > 0 {
                        if let Some(prev) = &expected_prev {
                            if block.previous_hash != *prev {
                                return;
                            }
                        } else {
                            return;
                        }
                    }
                    let mut new_chain = chain_snapshot.clone();
                    new_chain.push(block.clone());
                    let replayed = match crate::Blockchain::validate_chain_with_params(
                        &new_chain,
                        &params_snapshot,
                    ) {
                        Ok(state) => state,
                        Err(reason) => {
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason,
                                "chain_validation_failed"
                            );
                            return;
                        }
                    };
                    let import_state = match crate::Blockchain::build_chain_import_state(
                        new_chain,
                        &params_snapshot,
                        &replayed,
                    ) {
                        Ok(state) => state,
                        Err(err) => {
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason = %err,
                                "chain_import_state_failed"
                            );
                            return;
                        }
                    };
                    let mut bc = chain.guard();
                    if bc.chain.len() != current_len {
                        drop(bc);
                        continue;
                    }
                    if block.index > 0 {
                        let current_prev = bc.chain.last().map(|b| b.hash.clone());
                        if current_prev != expected_prev {
                            drop(bc);
                            continue;
                        }
                    } else if !bc.chain.is_empty() {
                        drop(bc);
                        continue;
                    }
                    if bc.params != params_snapshot {
                        drop(bc);
                        continue;
                    }
                    let lca = bc
                        .chain
                        .iter()
                        .zip(&import_state.chain)
                        .take_while(|(a, b)| a.hash == b.hash)
                        .count();
                    let depth = bc.chain.len().saturating_sub(lca);
                    let rollback_indices = bc
                        .chain
                        .iter()
                        .rev()
                        .take(depth)
                        .map(|b| b.index)
                        .collect::<Vec<_>>();
                    #[cfg(feature = "telemetry")]
                    let start = Instant::now();
                    let broadcast_chain = import_state.chain.clone();
                    let applied = bc.apply_import_state(import_state, replayed, &rollback_indices);
                    #[cfg(feature = "telemetry")]
                    observer::observe_convergence(start);
                    match applied {
                        Ok(()) => {
                            drop(bc);
                            self.schedule_chain_broadcast(broadcast_chain);
                        }
                        Err(err) => {
                            drop(bc);
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason = %err,
                                "chain_apply_failed"
                            );
                        }
                    }
                    return;
                }
            }
            Payload::Chain(new_chain) => {
                let authorized = self.is_authorized(&peer_key);
                if !authorized {
                    #[cfg(feature = "integration-tests")]
                    self.authorize(peer_key);
                    #[cfg(not(feature = "integration-tests"))]
                    {
                        diagnostics::tracing::warn!(
                            target = "net",
                            peer = %overlay_peer_label(&peer_key),
                            "chain_from_unauthorized_peer"
                        );
                        return;
                    }
                }
                let fast_mine = std::env::var("TB_FAST_MINE").as_deref() == Ok("1");
                let broadcast_chain = new_chain.clone();
                if fast_mine {
                    let mut bc = chain.guard();
                    let current_len = bc.chain.len();
                    if new_chain.len() > current_len {
                        #[cfg(feature = "telemetry")]
                        let start = Instant::now();
                        match bc.import_chain(new_chain.clone()) {
                            Ok(()) => {
                                drop(bc);
                                self.schedule_chain_broadcast(broadcast_chain.clone());
                                #[cfg(feature = "telemetry")]
                                observer::observe_convergence(start);
                                return;
                            }
                            Err(_err) => {
                                #[cfg(feature = "telemetry")]
                                observer::observe_convergence(start);
                            }
                        }
                    } else {
                        return;
                    }
                }
                let response_addr = addr_from_peer_key(&peer_key).or(addr);
                let new_len = new_chain.len();
                let (mut params_snapshot, mut chain_snapshot) = {
                    let bc = chain.guard();
                    (bc.params.clone(), bc.chain.clone())
                };
                loop {
                    let current_len = chain_snapshot.len();
                    if new_len <= current_len {
                        if new_len < current_len {
                            send_chain_snapshot(self, response_addr, chain_snapshot.clone());
                        }
                        return;
                    }
                    let lca = chain_snapshot
                        .iter()
                        .zip(&new_chain)
                        .take_while(|(a, b)| a.hash == b.hash)
                        .count();
                    let depth = current_len.saturating_sub(lca);
                    let rollback_indices = chain_snapshot
                        .iter()
                        .rev()
                        .take(depth)
                        .map(|b| b.index)
                        .collect::<Vec<_>>();
                    let replayed = match crate::Blockchain::validate_chain_with_params(
                        &new_chain,
                        &params_snapshot,
                    ) {
                        Ok(state) => state,
                        Err(reason) => {
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason,
                                "chain_validation_failed"
                            );
                            return;
                        }
                    };
                    let import_state = match crate::Blockchain::build_chain_import_state(
                        new_chain.clone(),
                        &params_snapshot,
                        &replayed,
                    ) {
                        Ok(state) => state,
                        Err(err) => {
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason = %err,
                                "chain_import_state_failed"
                            );
                            return;
                        }
                    };
                    let mut bc = chain.guard();
                    let chain_changed = bc.chain.len() != chain_snapshot.len()
                        || bc.chain.last().map(|b| b.hash.as_str())
                            != chain_snapshot.last().map(|b| b.hash.as_str());
                    if chain_changed || bc.params != params_snapshot {
                        params_snapshot = bc.params.clone();
                        chain_snapshot = bc.chain.clone();
                        drop(bc);
                        continue;
                    }
                    if new_len <= bc.chain.len() {
                        if new_len < bc.chain.len() {
                            let chain_snapshot = bc.chain.clone();
                            drop(bc);
                            send_chain_snapshot(self, response_addr, chain_snapshot);
                        }
                        return;
                    }
                    #[cfg(feature = "telemetry")]
                    let start = Instant::now();
                    let applied = bc.apply_import_state(import_state, replayed, &rollback_indices);
                    #[cfg(feature = "telemetry")]
                    {
                        observer::observe_convergence(start);
                        if start.elapsed() > Duration::from_millis(500) {
                            diagnostics::tracing::info!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                elapsed_ms = start.elapsed().as_millis(),
                                "chain_apply_slow"
                            );
                        }
                    }
                    match applied {
                        Ok(()) => {
                            drop(bc);
                            self.schedule_chain_broadcast(broadcast_chain.clone());
                        }
                        Err(err) => {
                            diagnostics::tracing::warn!(
                                target = "net",
                                peer = %overlay_peer_label(&peer_key),
                                reason = %err,
                                "chain_apply_failed"
                            );
                        }
                    }
                    return;
                }
            }
            Payload::ChainRequest(request) => {
                let current_len = {
                    let bc = chain.guard();
                    bc.chain.len() as u64
                };
                if current_len > request.from_height {
                    let chain_snapshot = {
                        let bc = chain.guard();
                        bc.chain.clone()
                    };
                    let target = addr_from_peer_key(&peer_key).or(addr);
                    send_chain_snapshot(self, target, chain_snapshot);
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
                    if matches!(code, PeerErrorCode::Banned | PeerErrorCode::RateLimit) {
                        if let Some(peer_addr) = addr {
                            let mut a = self.addrs.guard();
                            a.remove(&peer_addr);
                        }
                        self.authorized.guard().remove(&peer_key);
                    }
                    return;
                }
                let key = format!(
                    "chunk/{}/{}",
                    crypto_suite::hex::encode(chunk.root),
                    chunk.index
                );
                let _ = with_chunk_db(|db| db.try_insert(&key, chunk.data.into_vec()));
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
                            let label = if _applied { "applied" } else { "ignored" };
                            let labels = [label];
                            with_metric_handle(
                                "reputation_gossip_total",
                                &labels,
                                || {
                                    crate::telemetry::REPUTATION_GOSSIP_TOTAL
                                        .ensure_handle_for_label_values(&labels)
                                },
                                |counter| counter.inc(),
                            );
                            let latency = now_secs().saturating_sub(e.epoch) as f64;
                            crate::telemetry::REPUTATION_GOSSIP_LATENCY_SECONDS.observe(latency);
                            if !_applied {
                                crate::telemetry::REPUTATION_GOSSIP_FAIL_TOTAL.inc();
                            }
                        }
                    }
                }
            }
            Payload::StorageProviderAdvertisement(advert) => {
                provider_directory::handle_advertisement(advert);
            }
            Payload::StorageProviderLookup(request) => {
                provider_directory::handle_lookup_request(request, addr);
            }
            Payload::StorageProviderLookupResponse(response) => {
                provider_directory::handle_lookup_response(response);
            }
            Payload::StorageProviderQuery(request) => {
                provider_directory::handle_lookup_request(request, addr);
            }
            Payload::StorageProviderQueryResponse(response) => {
                provider_directory::handle_lookup_response(response);
            }
        }
    }
}

impl Default for PeerSet {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

struct PeerState {
    count: u32,
    rate_window_start: Instant,
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

/// Adaptive multi-factor peer reputation system
///
/// Tracks reputation across multiple behavioral dimensions and adapts
/// decay rates based on network health. Uses Network Health Index for context.
#[derive(Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    /// Composite reputation score [0.0, 1.0]
    pub score: f64,

    /// Component scores (tracked separately, then combined)
    #[serde(default)]
    pub message_validity: f64, // Valid messages vs invalid/malformed
    #[serde(default)]
    pub response_quality: f64, // Timely, complete responses
    #[serde(default)]
    pub resource_behavior: f64, // Bandwidth/connection fairness
    #[serde(default)]
    pub protocol_adherence: f64, // Follows protocol rules

    /// Infraction counters (for adaptive penalties)
    #[serde(default)]
    pub invalid_messages: u32,
    #[serde(default)]
    pub slow_responses: u32,
    #[serde(default)]
    pub protocol_violations: u32,

    #[serde(skip, default = "instant_now")]
    last_decay: Instant,

    #[serde(skip, default = "instant_now")]
    last_update: Instant,
}

impl Default for PeerReputation {
    fn default() -> Self {
        Self {
            score: 1.0,
            message_validity: 1.0,
            response_quality: 1.0,
            resource_behavior: 1.0,
            protocol_adherence: 1.0,
            invalid_messages: 0,
            slow_responses: 0,
            protocol_violations: 0,
            last_decay: instant_now(),
            last_update: instant_now(),
        }
    }
}

impl PeerReputation {
    /// Adaptive decay that adjusts based on network health
    ///
    /// In healthy networks: faster decay (forgive minor issues quickly)
    /// In unhealthy networks: slower decay (maintain strict standards longer)
    fn decay(&mut self, base_rate: f64, network_health: f64) {
        let elapsed = self.last_decay.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            // Adaptive decay rate based on network health
            // High health (0.8-1.0) → fast decay (forgive quickly)
            // Low health (0.0-0.5) → slow decay (stay vigilant)
            let health_factor = network_health.clamp(0.0, 1.0);
            let adaptive_rate = base_rate * (0.5 + health_factor); // Range: [0.5x, 1.5x] base rate

            // Apply exponential decay to component scores
            let decay_factor = (-adaptive_rate * elapsed).exp();

            // Decay components toward neutral (1.0), not zero
            self.message_validity = 1.0 - (1.0 - self.message_validity) * decay_factor;
            self.response_quality = 1.0 - (1.0 - self.response_quality) * decay_factor;
            self.resource_behavior = 1.0 - (1.0 - self.resource_behavior) * decay_factor;
            self.protocol_adherence = 1.0 - (1.0 - self.protocol_adherence) * decay_factor;

            // Decay infraction counters
            let counter_decay = 0.95_f64.powf(elapsed / 3600.0); // Half-life ~13 hours
            self.invalid_messages = (self.invalid_messages as f64 * counter_decay) as u32;
            self.slow_responses = (self.slow_responses as f64 * counter_decay) as u32;
            self.protocol_violations = (self.protocol_violations as f64 * counter_decay) as u32;

            self.recompute_composite();
            self.last_decay = Instant::now();
        }
    }

    /// Record invalid/malformed message
    #[cfg(test)]
    fn record_invalid_message(&mut self, network_health: f64) {
        self.invalid_messages = self.invalid_messages.saturating_add(1);

        // Adaptive penalty: more severe in healthy networks (likely malicious)
        // less severe in unhealthy networks (might be network issues)
        let base_penalty = 0.95; // 5% penalty
        let health_factor = network_health.clamp(0.0, 1.0);
        let adaptive_penalty = base_penalty + (1.0 - base_penalty) * (1.0 - health_factor) * 0.5;

        self.message_validity *= adaptive_penalty;
        self.message_validity = self.message_validity.max(0.1);

        self.recompute_composite();
        self.last_update = Instant::now();
    }

    /// Record slow/timed-out response
    #[cfg(test)]
    fn record_slow_response(&mut self, latency_ms: u64, network_health: f64) {
        self.slow_responses = self.slow_responses.saturating_add(1);

        // Penalty based on how slow the response was
        // <1s: no penalty, 1-5s: small penalty, >5s: large penalty
        let penalty_factor = if latency_ms < 1000 {
            1.0
        } else if latency_ms < 5000 {
            0.98 - 0.02 * ((latency_ms - 1000) as f64 / 4000.0)
        } else {
            0.90
        };

        // In unhealthy networks, be more lenient about slow responses
        let health_factor = network_health.clamp(0.0, 1.0);
        let adaptive_penalty =
            penalty_factor + (1.0 - penalty_factor) * (1.0 - health_factor) * 0.5;

        self.response_quality *= adaptive_penalty;
        self.response_quality = self.response_quality.max(0.1);

        self.recompute_composite();
        self.last_update = Instant::now();
    }

    /// Record protocol violation
    #[cfg(test)]
    fn record_protocol_violation(&mut self, severity: ViolationSeverity) {
        self.protocol_violations = self.protocol_violations.saturating_add(1);

        let penalty = match severity {
            ViolationSeverity::Minor => 0.98,    // 2% penalty
            ViolationSeverity::Moderate => 0.90, // 10% penalty
            ViolationSeverity::Severe => 0.70,   // 30% penalty
        };

        self.protocol_adherence *= penalty;
        self.protocol_adherence = self.protocol_adherence.max(0.1);

        self.recompute_composite();
        self.last_update = Instant::now();
    }

    /// Record good behavior (reward)
    #[cfg(test)]
    fn record_good_behavior(&mut self, category: BehaviorCategory, bonus: f64) {
        let reward = 1.0 + bonus.clamp(0.0, 0.1); // Max 10% boost per event

        match category {
            BehaviorCategory::MessageValidity => {
                self.message_validity = (self.message_validity * reward).min(1.0);
            }
            BehaviorCategory::ResponseQuality => {
                self.response_quality = (self.response_quality * reward).min(1.0);
            }
            BehaviorCategory::ResourceBehavior => {
                self.resource_behavior = (self.resource_behavior * reward).min(1.0);
            }
            BehaviorCategory::ProtocolAdherence => {
                self.protocol_adherence = (self.protocol_adherence * reward).min(1.0);
            }
        }

        self.recompute_composite();
        self.last_update = Instant::now();
    }

    /// Recompute composite score from components
    ///
    /// Uses weighted geometric mean to ensure all dimensions matter
    /// (one very bad component drags down the whole score)
    fn recompute_composite(&mut self) {
        // Weights (must sum to 1.0)
        const W_MSG: f64 = 0.3; // Message validity most important
        const W_RESP: f64 = 0.25; // Response quality
        const W_RES: f64 = 0.2; // Resource behavior
        const W_PROT: f64 = 0.25; // Protocol adherence

        // Weighted geometric mean
        self.score = (self.message_validity.powf(W_MSG)
            * self.response_quality.powf(W_RESP)
            * self.resource_behavior.powf(W_RES)
            * self.protocol_adherence.powf(W_PROT))
        .clamp(0.0, 1.0);
    }

    /// Check if peer should be banned based on reputation
    ///
    /// Adaptive thresholds based on network health:
    /// - Healthy network: stricter (ban at 0.3)
    /// - Unhealthy network: lenient (ban at 0.1)
    #[cfg(test)]
    fn should_ban(&self, network_health: f64) -> bool {
        let health_factor = network_health.clamp(0.0, 1.0);
        let ban_threshold = 0.1 + 0.2 * health_factor; // Range: [0.1, 0.3]

        self.score < ban_threshold
    }

    /// Get component with lowest score (for diagnostics)
    #[cfg(test)]
    fn weakest_component(&self) -> &'static str {
        let min = self
            .message_validity
            .min(self.response_quality)
            .min(self.resource_behavior)
            .min(self.protocol_adherence);

        if self.message_validity == min {
            "message_validity"
        } else if self.response_quality == min {
            "response_quality"
        } else if self.resource_behavior == min {
            "resource_behavior"
        } else {
            "protocol_adherence"
        }
    }

    /// Legacy penalize method (for backward compatibility)
    fn penalize(&mut self, penalty: f64) {
        // Distribute penalty across all components
        self.message_validity = (self.message_validity * penalty).max(0.1);
        self.response_quality = (self.response_quality * penalty).max(0.1);
        self.resource_behavior = (self.resource_behavior * penalty).max(0.1);
        self.protocol_adherence = (self.protocol_adherence * penalty).max(0.1);
        self.recompute_composite();
    }
}

/// Violation severity levels for adaptive penalties
#[derive(Debug, Clone, Copy)]
pub enum ViolationSeverity {
    Minor,    // Protocol deviation, recoverable
    Moderate, // Clear violation, potential attack attempt
    Severe,   // Critical violation, likely malicious
}

/// Behavior categories for rewards
#[derive(Debug, Clone, Copy)]
pub enum BehaviorCategory {
    MessageValidity,
    ResponseQuality,
    ResourceBehavior,
    ProtocolAdherence,
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

impl PeerErrorCode {
    #[cfg(feature = "telemetry")]
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
        let labels = [code.as_str()];
        with_metric_handle(
            "peer_error_total",
            &labels,
            || crate::telemetry::PEER_ERROR_TOTAL.ensure_handle_for_label_values(&labels),
            |counter| counter.inc(),
        );
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
                let labels = [r];
                with_metric_handle(
                    "peer_throttle_total",
                    &labels,
                    || {
                        crate::telemetry::PEER_THROTTLE_TOTAL
                            .ensure_handle_for_label_values(&labels)
                    },
                    |counter| counter.inc(),
                );
                with_metric_handle(
                    "peer_backpressure_active_total",
                    &labels,
                    || {
                        crate::telemetry::PEER_BACKPRESSURE_ACTIVE_TOTAL
                            .ensure_handle_for_label_values(&labels)
                    },
                    |counter| counter.inc(),
                );
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
                        let label = id.as_str();
                        let labels = [label];
                        let delta = bytes as u64 * sample as u64;
                        with_metric_handle(
                            "peer_bytes_sent_total",
                            &labels,
                            || {
                                crate::telemetry::PEER_BYTES_SENT_TOTAL
                                    .ensure_handle_for_label_values(&labels)
                            },
                            |counter| counter.inc_by(delta),
                        );
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
                        let label = id.as_str();
                        let labels = [label];
                        let delta = bytes as u64 * sample as u64;
                        with_metric_handle(
                            "peer_bytes_sent_total",
                            &labels,
                            || {
                                crate::telemetry::PEER_BYTES_SENT_TOTAL
                                    .ensure_handle_for_label_values(&labels)
                            },
                            |counter| counter.inc_by(delta),
                        );
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
                    let label = id.as_str();
                    let labels = [label];
                    with_metric_handle(
                        "peer_request_total",
                        &labels,
                        || {
                            crate::telemetry::PEER_REQUEST_TOTAL
                                .ensure_handle_for_label_values(&labels)
                        },
                        |counter| counter.inc_by(sample as u64),
                    );
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
                    let label = id.as_str();
                    let labels = [label];
                    with_metric_handle(
                        "peer_request_total",
                        &labels,
                        || {
                            crate::telemetry::PEER_REQUEST_TOTAL
                                .ensure_handle_for_label_values(&labels)
                        },
                        |counter| counter.inc_by(sample as u64),
                    );
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
        if matches!(reason, DropReason::TooBusy | DropReason::RateLimit) {
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
        if matches!(reason, DropReason::TooBusy | DropReason::RateLimit) {
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
            let label_peer = id.as_str();
            let reason_label = reason.as_ref();
            let labels = [label_peer, reason_label];
            with_metric_handle(
                "peer_drop_total",
                &labels,
                || crate::telemetry::PEER_DROP_TOTAL.ensure_handle_for_label_values(&labels),
                |counter| counter.inc(),
            );
        }
    }
    #[cfg(feature = "quic")]
    super::quic_stats::record_handshake_failure(pk);
}

fn record_handshake_issue(pk: &[u8; 32], reason: HandshakeError, penalize: bool) {
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
        if penalize {
            entry.reputation.penalize(0.95);
        }
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
        if penalize {
            entry.reputation.penalize(0.95);
        }
        entry.last_updated = now_secs();
        map.insert(*pk, entry);
        update_active_gauge(map.len());
        update_memory_usage(map.len());
    }
    #[cfg(feature = "telemetry")]
    {
        if EXPORT_PEER_METRICS.load(Ordering::Relaxed) {
            let id = overlay_peer_label(pk);
            let peer_label = id.as_str();
            let reason_label = reason.as_str();
            let peer_labels = [peer_label, reason_label];
            with_metric_handle(
                "peer_handshake_fail_total",
                &peer_labels,
                || {
                    crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&peer_labels)
                },
                |counter| counter.inc(),
            );
            let reason_only = [reason_label];
            with_metric_handle(
                "handshake_fail_total",
                &reason_only,
                || {
                    crate::telemetry::HANDSHAKE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&reason_only)
                },
                |counter| counter.inc(),
            );
            if matches!(reason, HandshakeError::Tls | HandshakeError::Certificate) {
                let tls_labels = [peer_label];
                with_metric_handle(
                    "peer_tls_error_total",
                    &tls_labels,
                    || {
                        crate::telemetry::PEER_TLS_ERROR_TOTAL
                            .ensure_handle_for_label_values(&tls_labels)
                    },
                    |counter| counter.inc(),
                );
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

fn record_handshake_fail(pk: &[u8; 32], reason: HandshakeError) {
    record_handshake_issue(pk, reason, true);
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
        let labels = [id.as_str()];
        with_metric_handle(
            "peer_handshake_success_total",
            &labels,
            || {
                crate::telemetry::PEER_HANDSHAKE_SUCCESS_TOTAL
                    .ensure_handle_for_label_values(&labels)
            },
            |counter| counter.inc(),
        );
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

pub fn addr_for_pk(pk: &[u8; 32]) -> Option<SocketAddr> {
    let map = ADDR_MAP.guard();
    map.iter().find_map(
        |(addr, stored)| {
            if stored == pk {
                Some(*addr)
            } else {
                None
            }
        },
    )
}

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
            let labels = [id.as_str()];
            with_metric_handle(
                "peer_reputation_score",
                &labels,
                || crate::telemetry::PEER_REPUTATION_SCORE.ensure_handle_for_label_values(&labels),
                |gauge| gauge.set(score),
            );
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
                let labels = [id.as_str()];
                with_metric_handle(
                    "peer_stats_reset_total",
                    &labels,
                    || {
                        crate::telemetry::PEER_STATS_RESET_TOTAL
                            .ensure_handle_for_label_values(&labels)
                    },
                    |counter| counter.inc(),
                );
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
    let mut metrics = {
        let mut map = peer_metrics_guard();
        map.shift_remove_entry(old).map(|(_, metrics)| metrics)
    };

    if metrics.is_none() {
        // If metrics were reloaded from disk after a config change, try to
        // refresh and recover the peer entry before refusing rotation.
        load_peer_metrics();
        let mut map = peer_metrics_guard();
        metrics = map.shift_remove_entry(old).map(|(_, metrics)| metrics);
    }

    if metrics.is_none() {
        let ids = PEER_IDENTITIES.guard();
        if !ids.contains_key(old) {
            return false;
        }
        // Minimum viable metrics: the peer has successfully handshaked at least once.
        let mut fallback = PeerMetrics::default();
        fallback.requests = 1;
        fallback.handshake_success = 1;
        fallback.last_updated = now_secs();
        metrics = Some(fallback);
    }

    if let Some(metrics) = metrics {
        let mut map = peer_metrics_guard();
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
                if let Err(err) = std::fs::create_dir_all(parent) {
                    diagnostics::tracing::warn!(
                        path = %parent.display(),
                        %err,
                        "failed to create key history directory"
                    );
                }
            }
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let mut entry = Map::new();
                    entry.insert(
                        "old".to_owned(),
                        Value::String(crypto_suite::hex::encode(old)),
                    );
                    entry.insert(
                        "new".to_owned(),
                        Value::String(crypto_suite::hex::encode(new)),
                    );
                    entry.insert("ts".to_owned(), Value::from(now_secs()));
                    let value = Value::Object(entry);
                    if let Err(err) = writeln!(file, "{}", json::to_string_value(&value)) {
                        diagnostics::tracing::warn!(
                            path = %path,
                            %err,
                            "failed to append key history entry"
                        );
                    }
                }
                Err(err) => {
                    diagnostics::tracing::warn!(
                        path = %path,
                        %err,
                        "failed to open key history log"
                    );
                }
            }
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
        let labels = [id.as_str()];
        with_metric_handle(
            "peer_stats_query_total",
            &labels,
            || crate::telemetry::PEER_STATS_QUERY_TOTAL.ensure_handle_for_label_values(&labels),
            |counter| counter.inc(),
        );
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
        let tmp_dir = sys::tempfile::tempdir_in(&dir).map_err(sys_to_io_error)?;
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
            let labels = [label];
            with_metric_handle(
                "peer_stats_export_total",
                &labels,
                || {
                    crate::telemetry::PEER_STATS_EXPORT_TOTAL
                        .ensure_handle_for_label_values(&labels)
                },
                |counter| counter.inc(),
            );
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
            let tmp_dir = sys::tempfile::tempdir_in(&base).map_err(sys_to_io_error)?;
            let mut tmp = NamedTempFile::new_in(tmp_dir.path()).map_err(sys_to_io_error)?;
            tmp.as_file().lock_exclusive().map_err(sys_to_io_error)?;

            let mut tar_builder = tar::Builder::new(Vec::new());
            for pk in &keys {
                let m = {
                    let map = peer_metrics_guard();
                    match map.get(pk) {
                        Some(v) => v.clone(),
                        None => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "peer list changed",
                            ));
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
                tar_builder.append_data(&mut header, format!("{id}.json"), data.as_slice())?;
            }
            let tar_bytes = tar_builder.finish()?;
            let gz_bytes = gzip::encode(&tar_bytes);
            tmp.write_all(&gz_bytes)?;
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

            if let Err(err) = validate_metrics_archive(&path) {
                #[cfg(feature = "telemetry")]
                {
                    let labels = ["error"];
                    with_metric_handle(
                        "peer_stats_export_validate_total",
                        &labels,
                        || {
                            crate::telemetry::PEER_STATS_EXPORT_VALIDATE_TOTAL
                                .ensure_handle_for_label_values(&labels)
                        },
                        |counter| counter.inc(),
                    );
                    diagnostics::tracing::warn!(
                        path = %path.display(),
                        error = ?err,
                        "peer metrics archive validation failed",
                    );
                }
                return Err(err);
            }
            #[cfg(feature = "telemetry")]
            {
                let labels = ["ok"];
                with_metric_handle(
                    "peer_stats_export_validate_total",
                    &labels,
                    || {
                        crate::telemetry::PEER_STATS_EXPORT_VALIDATE_TOTAL
                            .ensure_handle_for_label_values(&labels)
                    },
                    |counter| counter.inc(),
                );
            }
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
                            ));
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
            let labels = [label];
            with_metric_handle(
                "peer_stats_export_all_total",
                &labels,
                || {
                    crate::telemetry::PEER_STATS_EXPORT_ALL_TOTAL
                        .ensure_handle_for_label_values(&labels)
                },
                |counter| counter.inc(),
            );
        }
    }

    res
}

#[derive(Clone)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: PeerMetrics,
}

pub(crate) fn peer_stats_to_json(stats: &[PeerStat]) -> Value {
    Value::Array(stats.iter().map(peer_stat_to_value).collect())
}

fn peer_stat_to_value(stat: &PeerStat) -> Value {
    let mut map = Map::new();
    map.insert("peer_id".to_owned(), Value::String(stat.peer_id.clone()));
    map.insert("metrics".to_owned(), peer_metrics_to_value(&stat.metrics));
    Value::Object(map)
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
            let peer_label = id.as_str();
            let labels = [peer_label, DropReason::Duplicate.as_ref()];
            with_metric_handle(
                "peer_drop_total",
                &labels,
                || crate::telemetry::PEER_DROP_TOTAL.ensure_handle_for_label_values(&labels),
                |counter| counter.inc(),
            );
        }
    }
    #[cfg(test)]
    {
        RECORDED_DROPS.guard().push(*ip);
    }
}

fn peer_db_path_from_env() -> PathBuf {
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
    cert: Bytes,
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::Value;
    #[cfg(feature = "telemetry")]
    use runtime::telemetry::MetricError;
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

    #[cfg(feature = "telemetry")]
    #[test]
    fn telemetry_helpers_warn_and_skip_on_missing_labels() {
        use diagnostics::internal::install_subscriber;
        use std::sync::{Arc, Mutex};

        let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let capture = Arc::clone(&messages);
        let guard = install_subscriber(move |record| {
            if record.target.as_ref() == "telemetry" {
                if let Ok(mut logs) = capture.lock() {
                    logs.push(record.message.to_string());
                }
            }
        });
        let executed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&executed);
        super::with_metric_handle(
            "test_metric",
            &["peer"],
            || Err(MetricError::MissingLabelSet),
            move |_: &()| {
                flag.store(true, Ordering::SeqCst);
            },
        );
        drop(guard);
        let logs = messages.lock().unwrap_or_else(|e| e.into_inner());
        assert!(logs
            .iter()
            .any(|msg| msg.contains("failed to obtain telemetry handle")));
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[test]
    fn aggregator_failover_selects_next_url() {
        runtime::block_on(async {
            let received = Arc::new(AtomicBool::new(false));
            let _guard = TestIngestGuard::new(received.clone());
            let bad = "test://fail".to_string();
            let good = "test://success".to_string();
            let client = AggregatorClient::new(vec![bad, good], "t".into());
            aggregator_guard().replace(client.clone());
            let snap = PeerSnapshot {
                peer_id: "p".into(),
                metrics: PeerMetrics::default(),
            };
            client.ingest(vec![snap]).await;
            assert!(received.load(Ordering::SeqCst));
            let payload = {
                let guard = super::TEST_PAYLOADS.guard();
                guard.last().cloned().expect("captured payload")
            };
            if let Value::Array(items) = payload {
                assert_eq!(items.len(), 1);
                if let Value::Object(obj) = &items[0] {
                    assert!(obj.contains_key("peer_id"));
                    assert!(obj.contains_key("metrics"));
                } else {
                    panic!("expected metrics object");
                }
            } else {
                panic!("expected array payload");
            }
            *aggregator_guard() = None;
        });
    }

    #[test]
    fn peer_snapshot_array_matches_legacy_serialization() {
        let snapshot = PeerSnapshot {
            peer_id: "peer".into(),
            metrics: PeerMetrics::default(),
        };
        let expected = Value::Array(vec![peer_snapshot_to_value(&snapshot)]);
        let actual = peer_snapshot_array(std::slice::from_ref(&snapshot));
        assert_eq!(actual, expected);
    }

    struct TestIngestGuard;

    impl TestIngestGuard {
        fn new(flag: Arc<AtomicBool>) -> Self {
            *super::TEST_INGEST.guard() = Some(flag);
            super::TEST_PAYLOADS.guard().clear();
            Self
        }
    }

    impl Drop for TestIngestGuard {
        fn drop(&mut self) {
            *super::TEST_INGEST.guard() = None;
            super::TEST_PAYLOADS.guard().clear();
        }
    }

    // Adaptive Peer Reputation Tests

    #[test]
    fn test_reputation_adaptive_decay() {
        // Test that decay rate adapts based on network health
        let mut rep = PeerReputation::default();

        // Penalize to bring score down
        rep.message_validity = 0.5;
        rep.recompute_composite();
        let _initial_score = rep.score;

        // In healthy network (0.9), decay should be faster
        std::thread::sleep(std::time::Duration::from_millis(100));
        rep.decay(0.01, 0.9);
        let healthy_network_score = rep.score;

        // Reset for comparison
        rep.message_validity = 0.5;
        rep.recompute_composite();
        rep.last_decay = instant_now();

        // In unhealthy network (0.2), decay should be slower
        std::thread::sleep(std::time::Duration::from_millis(100));
        rep.decay(0.01, 0.2);
        let unhealthy_network_score = rep.score;

        // Healthy network should recover faster (closer to 1.0)
        assert!(
            healthy_network_score > unhealthy_network_score,
            "Healthy network ({}) should forgive faster than unhealthy ({})",
            healthy_network_score,
            unhealthy_network_score
        );
    }

    #[test]
    fn test_reputation_multi_factor_scoring() {
        let mut rep = PeerReputation::default();

        // Perfect in 3 dimensions, terrible in 1
        rep.message_validity = 1.0;
        rep.response_quality = 1.0;
        rep.resource_behavior = 0.2; // Bad resource behavior
        rep.protocol_adherence = 1.0;
        rep.recompute_composite();

        // Geometric mean should drag composite down (not arithmetic average)
        // With weights: 0.3, 0.25, 0.2, 0.25
        // GM = 1.0^0.3 × 1.0^0.25 × 0.2^0.2 × 1.0^0.25 = 0.2^0.2 ≈ 0.725
        assert!(
            rep.score < 0.8,
            "One bad component should drag down composite: got {}",
            rep.score
        );
        assert!(
            rep.score > 0.6,
            "But not too much with low weight: got {}",
            rep.score
        );
    }

    #[test]
    fn test_reputation_adaptive_penalties() {
        // Test that penalties adapt based on network health

        // Healthy network: invalid messages penalized more (likely malicious)
        let mut rep_healthy = PeerReputation::default();
        rep_healthy.record_invalid_message(0.9);
        let score_healthy = rep_healthy.score;

        // Unhealthy network: invalid messages penalized less (might be network issues)
        let mut rep_unhealthy = PeerReputation::default();
        rep_unhealthy.record_invalid_message(0.2);
        let score_unhealthy = rep_unhealthy.score;

        assert!(
            score_healthy < score_unhealthy,
            "Healthy network ({}) should penalize more than unhealthy ({})",
            score_healthy,
            score_unhealthy
        );
    }

    #[test]
    fn test_reputation_slow_response_scoring() {
        let mut rep = PeerReputation::default();

        // Fast response (<1s): no penalty
        rep.record_slow_response(500, 0.8);
        assert_eq!(
            rep.response_quality, 1.0,
            "Fast response should not be penalized"
        );

        // Slow response (3s): moderate penalty
        rep.record_slow_response(3000, 0.8);
        assert!(
            rep.response_quality < 1.0,
            "Slow response should be penalized"
        );
        assert!(
            rep.response_quality > 0.9,
            "Moderate slowness gets moderate penalty"
        );

        // Very slow response (10s): large penalty
        rep.record_slow_response(10000, 0.8);
        assert!(
            rep.response_quality < 0.9,
            "Very slow response heavily penalized"
        );
    }

    #[test]
    fn test_reputation_protocol_violation_severity() {
        let mut rep = PeerReputation::default();

        // Minor violation
        rep.record_protocol_violation(ViolationSeverity::Minor);
        let score_minor = rep.protocol_adherence;

        rep.protocol_adherence = 1.0; // Reset

        // Severe violation
        rep.record_protocol_violation(ViolationSeverity::Severe);
        let score_severe = rep.protocol_adherence;

        assert!(
            score_severe < score_minor,
            "Severe violations ({}) should penalize more than minor ({})",
            score_severe,
            score_minor
        );
    }

    #[test]
    fn test_reputation_adaptive_ban_threshold() {
        let mut rep = PeerReputation::default();
        rep.score = 0.2; // Borderline reputation

        // Healthy network: stricter threshold (ban at 0.3)
        assert!(
            rep.should_ban(0.9),
            "Should ban with score 0.2 in healthy network (threshold 0.3)"
        );

        // Unhealthy network: lenient threshold (ban at 0.1)
        assert!(
            !rep.should_ban(0.0),
            "Should NOT ban with score 0.2 in unhealthy network (threshold 0.1)"
        );
    }

    #[test]
    fn test_reputation_good_behavior_rewards() {
        let mut rep = PeerReputation::default();

        // Start with degraded reputation
        rep.message_validity = 0.8;
        rep.recompute_composite();
        let initial_score = rep.score;

        // Reward good behavior
        rep.record_good_behavior(BehaviorCategory::MessageValidity, 0.05);

        assert!(
            rep.message_validity > 0.8,
            "Good behavior should improve component"
        );
        assert!(
            rep.score > initial_score,
            "Good behavior should improve composite"
        );
        assert!(rep.message_validity <= 1.0, "Rewards should not exceed max");
    }

    #[test]
    fn test_reputation_weakest_component_identification() {
        let mut rep = PeerReputation::default();

        rep.message_validity = 0.3; // Weakest
        rep.response_quality = 0.9;
        rep.resource_behavior = 0.8;
        rep.protocol_adherence = 0.7;

        assert_eq!(rep.weakest_component(), "message_validity");

        rep.message_validity = 1.0;
        rep.response_quality = 0.2; // Now weakest

        assert_eq!(rep.weakest_component(), "response_quality");
    }

    #[test]
    fn test_reputation_infraction_counter_decay() {
        let mut rep = PeerReputation::default();

        // Record some infractions
        rep.invalid_messages = 100;
        rep.slow_responses = 50;
        rep.protocol_violations = 30;

        // Fast-forward time (simulate 7 hours elapsed)
        rep.last_decay = Instant::now() - std::time::Duration::from_secs(7 * 3600);

        // Decay
        rep.decay(0.01, 0.5);

        // Counters should have decayed significantly
        assert!(
            rep.invalid_messages < 100,
            "Invalid message count should decay"
        );
        assert!(rep.slow_responses < 50, "Slow response count should decay");
        assert!(
            rep.protocol_violations < 30,
            "Protocol violation count should decay"
        );
    }
}

fn quic_peer_db_path_from_env() -> PathBuf {
    std::env::var("TB_QUIC_PEER_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("quic_peers.txt")
        })
}

fn load_quic_peers(path: &Path) -> HashMap<SocketAddr, QuicEndpoint> {
    use base64_fp::decode_standard;
    let mut map = HashMap::new();
    if let Ok(data) = fs::read_to_string(path) {
        for line in data.lines() {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() == 3 {
                if let (Ok(tcp), Ok(quic)) = (parts[0].parse(), parts[1].parse()) {
                    if let Ok(cert) = decode_standard(parts[2]) {
                        map.insert(
                            tcp,
                            QuicEndpoint {
                                addr: quic,
                                cert: Bytes::from(cert),
                            },
                        );
                    }
                }
            }
        }
    }
    map
}

fn persist_quic_peers(path: &Path, map: &HashMap<SocketAddr, QuicEndpoint>) {
    use base64_fp::encode_standard;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = map
        .iter()
        .map(|(tcp, info)| {
            format!(
                "{tcp},{},{}",
                info.addr,
                encode_standard(info.cert.as_ref())
            )
        })
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

struct ChunkDb {
    path: PathBuf,
    db: SimpleDb,
}

fn open_chunk_db(path: &Path) -> SimpleDb {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let path_string = path.to_string_lossy().into_owned();
    SimpleDb::open_named(names::NET_PEER_CHUNKS, &path_string)
}

fn with_chunk_db<F, R>(f: F) -> R
where
    F: FnOnce(&mut SimpleDb) -> R,
{
    let desired = chunk_db_path();
    let mut guard = CHUNK_DB.guard();
    if guard.path != desired {
        guard.db = open_chunk_db(&desired);
        guard.path = desired;
    }
    f(&mut guard.db)
}

static CHUNK_DB: Lazy<Mutex<ChunkDb>> = Lazy::new(|| {
    let path = chunk_db_path();
    let db = open_chunk_db(&path);
    Mutex::new(ChunkDb { path, db })
});

fn persist_peers(path: &Path, set: &HashSet<SocketAddr>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut addrs: Vec<String> = set.iter().map(|a| a.to_string()).collect();
    addrs.sort();
    let _ = fs::write(path, addrs.join("\n"));
}

pub fn known_peers() -> Vec<SocketAddr> {
    let path = peer_db_path_from_env();
    if let Ok(data) = fs::read_to_string(path) {
        data.lines().filter_map(|l| l.parse().ok()).collect()
    } else {
        Vec::new()
    }
}

/// Return known peers with transport information and optional QUIC certificates.
pub fn known_peers_with_info() -> Vec<(SocketAddr, Transport, Option<Bytes>)> {
    PeerSet::new(Vec::new()).list_with_info()
}

static P2P_MAX_PER_SEC: Lazy<AtomicU32> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_MAX_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    AtomicU32::new(val)
});

static P2P_RATE_WINDOW_SECS: Lazy<std::sync::atomic::AtomicU64> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_RATE_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    std::sync::atomic::AtomicU64::new(val)
});

static P2P_CHAIN_SYNC_INTERVAL_MS: Lazy<AtomicU64> = Lazy::new(|| {
    let val = std::env::var("TB_P2P_CHAIN_SYNC_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    AtomicU64::new(val)
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

static PEER_METRICS: Lazy<Mutex<OrderedMap<[u8; 32], PeerMetrics>>> =
    Lazy::new(|| Mutex::new(OrderedMap::new()));

fn peer_metrics_guard() -> std::sync::MutexGuard<'static, OrderedMap<[u8; 32], PeerMetrics>> {
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

#[derive(Clone, Serialize)]
pub struct PeerSnapshot {
    pub peer_id: String,
    pub metrics: PeerMetrics,
}

static METRIC_TX: Lazy<broadcast::Sender<PeerSnapshot>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(1024);
    tx
});

fn peer_snapshot_array(snaps: &[PeerSnapshot]) -> Value {
    Value::Array(snaps.iter().map(peer_snapshot_to_value).collect())
}

fn peer_snapshot_to_value(snapshot: &PeerSnapshot) -> Value {
    let mut map = Map::new();
    map.insert(
        "peer_id".to_owned(),
        Value::String(snapshot.peer_id.clone()),
    );
    map.insert(
        "metrics".to_owned(),
        peer_metrics_to_value(&snapshot.metrics),
    );
    Value::Object(map)
}

fn peer_metrics_to_value(metrics: &PeerMetrics) -> Value {
    let mut map = Map::new();
    map.insert("requests".to_owned(), Value::from(metrics.requests));
    map.insert("bytes_sent".to_owned(), Value::from(metrics.bytes_sent));
    map.insert("sends".to_owned(), Value::from(metrics.sends));
    map.insert(
        "drops".to_owned(),
        counts_to_object(&metrics.drops, |reason| reason.as_ref().to_owned()),
    );
    map.insert(
        "handshake_fail".to_owned(),
        counts_to_object(&metrics.handshake_fail, |err| err.as_str().to_owned()),
    );
    map.insert(
        "handshake_success".to_owned(),
        Value::from(metrics.handshake_success),
    );
    map.insert(
        "last_handshake_ms".to_owned(),
        Value::from(metrics.last_handshake_ms),
    );
    map.insert("tls_errors".to_owned(), Value::from(metrics.tls_errors));
    map.insert(
        "reputation".to_owned(),
        peer_reputation_to_value(&metrics.reputation),
    );
    map.insert("last_updated".to_owned(), Value::from(metrics.last_updated));
    map.insert("req_avg".to_owned(), Value::from(metrics.req_avg));
    map.insert("byte_avg".to_owned(), Value::from(metrics.byte_avg));
    map.insert(
        "throttled_until".to_owned(),
        Value::from(metrics.throttled_until),
    );
    map.insert(
        "throttle_reason".to_owned(),
        metrics
            .throttle_reason
            .as_ref()
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "backoff_level".to_owned(),
        Value::from(metrics.backoff_level),
    );
    map.insert("sec_start".to_owned(), Value::from(metrics.sec_start));
    Value::Object(map)
}

fn peer_reputation_to_value(rep: &PeerReputation) -> Value {
    let mut map = Map::new();
    map.insert("score".to_owned(), Value::from(rep.score));
    Value::Object(map)
}

fn counts_to_object<K, F>(counts: &HashMap<K, u64>, key_fn: F) -> Value
where
    K: std::cmp::Eq + std::hash::Hash,
    F: Fn(&K) -> String,
{
    let mut map = Map::new();
    for (key, value) in counts {
        map.insert(key_fn(key), Value::from(*value));
    }
    Value::Object(map)
}

#[derive(Clone)]
struct AggregatorClient {
    urls: Vec<String>,
    token: String,
    client: httpd::HttpClient,
    idx: Arc<AtomicUsize>,
    handle: runtime::RuntimeHandle,
}

#[cfg(test)]
pub(super) static TEST_INGEST: Lazy<Mutex<Option<Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(None));
#[cfg(test)]
pub(super) static TEST_PAYLOADS: Lazy<Mutex<Vec<Value>>> = Lazy::new(|| Mutex::new(Vec::new()));

impl AggregatorClient {
    fn new(urls: Vec<String>, token: String) -> Self {
        Self {
            urls,
            token,
            client: http_client::http_client(),
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
        let body = match json::to_value(&snaps) {
            Ok(value) => value,
            Err(_) => peer_snapshot_array(&snaps),
        };
        self.post("ingest", body).await;
    }

    #[cfg(feature = "telemetry")]
    async fn telemetry_summary(&self, summary: crate::telemetry::summary::TelemetrySummary) {
        let body = match json::to_value(summary) {
            Ok(value) => value,
            Err(err) => {
                diagnostics::tracing::warn!(
                    %err,
                    "failed to encode telemetry summary for aggregator"
                );
                Value::Null
            }
        };
        self.post("telemetry", body).await;
    }

    async fn post(&self, path: &str, body: Value) {
        for i in 0..self.urls.len() {
            let idx = (self.idx.load(Ordering::Relaxed) + i) % self.urls.len();
            let base = self.urls[idx].trim_end_matches('/');
            let url = format!("{base}/{path}");
            let uri = match httpd::Uri::parse(&url) {
                Ok(uri) => uri,
                Err(_) => continue,
            };

            #[cfg(test)]
            if uri.scheme() == "test" {
                if test_post(&uri, &body).is_ok() {
                    self.idx.store(idx, Ordering::Relaxed);
                    break;
                }
                continue;
            }

            if uri.scheme() != "http" && uri.scheme() != "https" {
                continue;
            }

            let request = match self.client.request(httpd::Method::Post, &url) {
                Ok(builder) => builder.header("x-auth-token", self.token.clone()),
                Err(_) => continue,
            };
            let request = match request.json(&body) {
                Ok(builder) => builder,
                Err(_) => continue,
            };
            if request.send().await.is_ok() {
                self.idx.store(idx, Ordering::Relaxed);
                break;
            }
        }
    }
}

#[cfg(test)]
fn test_post(uri: &httpd::Uri, body: &Value) -> Result<(), ()> {
    match uri.host_str() {
        Some("success") => {
            TEST_PAYLOADS.guard().push(body.clone());
            if let Some(flag) = TEST_INGEST.guard().clone() {
                flag.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
        _ => Err(()),
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

fn log_ban_store_error(context: &str, pk: &[u8; 32], err: &BanStoreError) {
    let peer = overlay_peer_label(pk);
    diagnostics::tracing::warn!(
        target: "net",
        %peer,
        %context,
        %err,
        "ban store operation failed"
    );
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
        let mut metrics = Map::new();
        metrics.insert(
            "key_rotation".to_owned(),
            Value::String(crypto_suite::hex::encode(new)),
        );
        let mut event = Map::new();
        event.insert("peer_id".to_owned(), Value::String(overlay_peer_label(old)));
        event.insert("metrics".to_owned(), Value::Object(metrics));
        let payload = Value::Array(vec![Value::Object(event)]);
        let fut_client = client.clone();
        client.spawn(async move {
            fut_client.post("ingest", payload).await;
        });
    }
}

#[cfg(feature = "telemetry")]
const DROP_REASON_VARIANTS: &[DropReason] = &[
    DropReason::RateLimit,
    DropReason::Malformed,
    DropReason::Blacklist,
    DropReason::Duplicate,
    DropReason::TooBusy,
    DropReason::Other,
];

#[cfg(feature = "telemetry")]
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

pub fn set_p2p_rate_window_secs(v: u64) {
    P2P_RATE_WINDOW_SECS.store(v, Ordering::Relaxed);
}

pub fn set_p2p_chain_sync_interval_ms(v: u64) {
    P2P_CHAIN_SYNC_INTERVAL_MS.store(v, Ordering::Relaxed);
}

pub fn set_p2p_max_bytes_per_sec(v: u64) {
    P2P_MAX_BYTES_PER_SEC.store(v, Ordering::Relaxed);
}

pub fn p2p_max_bytes_per_sec() -> u64 {
    P2P_MAX_BYTES_PER_SEC.load(Ordering::Relaxed)
}

pub fn p2p_chain_sync_interval_ms() -> u64 {
    P2P_CHAIN_SYNC_INTERVAL_MS.load(Ordering::Relaxed)
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
        let labels = [reason];
        with_metric_handle(
            "peer_throttle_total",
            &labels,
            || crate::telemetry::PEER_THROTTLE_TOTAL.ensure_handle_for_label_values(&labels),
            |counter| counter.inc(),
        );
        with_metric_handle(
            "peer_backpressure_active_total",
            &labels,
            || {
                crate::telemetry::PEER_BACKPRESSURE_ACTIVE_TOTAL
                    .ensure_handle_for_label_values(&labels)
            },
            |counter| counter.inc(),
        );
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

fn maybe_consolidate(map: &mut OrderedMap<[u8; 32], PeerMetrics>) {
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

fn evict_lru(map: &mut OrderedMap<[u8; 32], PeerMetrics>) -> Option<[u8; 32]> {
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
        let label = id.as_str();
        let labels = [label];
        with_metric_handle(
            "peer_request_total",
            &labels,
            || crate::telemetry::PEER_REQUEST_TOTAL.ensure_handle_for_label_values(&labels),
            |counter| counter.inc_by(m.requests),
        );
        with_metric_handle(
            "peer_bytes_sent_total",
            &labels,
            || crate::telemetry::PEER_BYTES_SENT_TOTAL.ensure_handle_for_label_values(&labels),
            |counter| counter.inc_by(m.bytes_sent),
        );
        for (r, c) in &m.drops {
            let drop_labels = [label, r.as_ref()];
            with_metric_handle(
                "peer_drop_total",
                &drop_labels,
                || crate::telemetry::PEER_DROP_TOTAL.ensure_handle_for_label_values(&drop_labels),
                |counter| counter.inc_by(*c),
            );
        }
        for (r, c) in &m.handshake_fail {
            let handshake_labels = [label, r.as_str()];
            with_metric_handle(
                "peer_handshake_fail_total",
                &handshake_labels,
                || {
                    crate::telemetry::PEER_HANDSHAKE_FAIL_TOTAL
                        .ensure_handle_for_label_values(&handshake_labels)
                },
                |counter| counter.inc_by(*c),
            );
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
        let is_empty = map.is_empty();
        if is_empty {
            map.clear();
        }
        for (pk, m) in entries {
            let should_update = if is_empty {
                true
            } else {
                match map.get(&pk) {
                    Some(existing) => m.last_updated > existing.last_updated,
                    None => true,
                }
            };
            if should_update {
                if export {
                    register_peer_metrics(&pk, &m);
                }
                map.insert(pk, m);
            }
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
