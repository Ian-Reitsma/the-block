use crate::gossip::config::{self, GossipConfig};
use crate::net::partition_watch::PARTITION_WATCH;
use crate::net::peer::{pk_from_addr, PeerMetrics};
use crate::net::{
    overlay_peer_from_base58, overlay_peer_from_bytes, overlay_peer_to_base58, peer_stats_map,
    send_msg, send_quic_msg, Message, OverlayPeerId, Transport,
};
use crate::simple_db::{names, SimpleDb};
use codec::profiles;
use concurrency::cache::LruCache;
use concurrency::{Bytes, MutexExt, MutexGuard};
use crypto_suite::hashing::blake3::hash;
#[cfg(feature = "telemetry")]
use diagnostics::log;
#[cfg(test)]
use diagnostics::tracing;
use foundation_serialization::Serialize;
use ledger::address::ShardId;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    GOSSIP_DUPLICATE_TOTAL, GOSSIP_FANOUT_GAUGE, GOSSIP_LATENCY_BUCKETS, GOSSIP_PEER_FAILURE_TOTAL,
    GOSSIP_TTL_DROP_TOTAL,
};

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
        Err(err) => log::warn!(
            "metric_label_registration_failed: metric={metric} labels={labels:?} err={err}"
        ),
    }
}

use crate::range_boost;
use p2p_overlay::PeerId;

#[derive(Clone)]
struct GossipSettings {
    ttl: Duration,
    dedup_capacity: NonZeroUsize,
    min_fanout: usize,
    base_fanout: usize,
    max_fanout: usize,
    failure_penalty: f64,
    latency_weight: f64,
    reputation_weight: f64,
    latency_baseline_ms: f64,
    low_score_cutoff: f64,
    shard_store_path: String,
}

impl From<GossipConfig> for GossipSettings {
    fn from(cfg: GossipConfig) -> Self {
        Self {
            ttl: Duration::from_millis(cfg.ttl_ms.max(1)),
            dedup_capacity: cfg.dedup_capacity(),
            min_fanout: cfg.min_fanout,
            base_fanout: cfg.base_fanout,
            max_fanout: cfg.max_fanout,
            failure_penalty: cfg.failure_penalty,
            latency_weight: cfg.latency_weight,
            reputation_weight: cfg.reputation_weight,
            latency_baseline_ms: cfg.latency_baseline_ms as f64,
            low_score_cutoff: cfg.low_score_cutoff,
            shard_store_path: cfg.shard_store_path,
        }
    }
}

struct ShardStore {
    db: Mutex<SimpleDb>,
    cache: Mutex<HashMap<ShardId, Vec<OverlayPeerId>>>,
}

impl ShardStore {
    fn with_factory<F>(path: &str, factory: &F) -> Self
    where
        F: Fn(&str, &str) -> SimpleDb,
    {
        let db = factory(names::GOSSIP_RELAY, path);
        let cache = Mutex::new(Self::load(&db));
        Self {
            db: Mutex::new(db),
            cache,
        }
    }

    #[cfg(test)]
    fn temporary() -> Self {
        match sys::tempfile::tempdir().map(|dir| dir.into_path()) {
            Ok(base) => {
                let path = base.join("gossip_store");
                let path_str = path.to_string_lossy().to_string();
                Self::with_factory(&path_str, &SimpleDb::open_named)
            }
            Err(err) => {
                tracing::warn!(reason = %err, "gossip_shard_tempdir_fallback");
                let mut fallback = std::env::temp_dir();
                fallback.push(format!(
                    "the-block-gossip-shard-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or_default()
                ));
                match std::fs::create_dir_all(&fallback) {
                    Ok(()) => {
                        let path = fallback.join("gossip_store");
                        let path_str = path.to_string_lossy().to_string();
                        Self::with_factory(&path_str, &SimpleDb::open_named)
                    }
                    Err(create_err) => {
                        tracing::error!(
                            reason = %create_err,
                            "gossip_shard_tempdir_fallback_failed"
                        );
                        let db = SimpleDb::default();
                        let cache = Mutex::new(Self::load(&db));
                        Self {
                            db: Mutex::new(db),
                            cache,
                        }
                    }
                }
            }
        }
    }

    fn load(db: &SimpleDb) -> HashMap<ShardId, Vec<OverlayPeerId>> {
        let mut out = HashMap::new();
        for key in db.keys_with_prefix("shard:") {
            if let Some(suffix) = key.strip_prefix("shard:") {
                if let Ok(shard) = suffix.parse::<ShardId>() {
                    if let Some(bytes) = db.get(&key) {
                        if let Ok(mut peers) = codec::deserialize::<Vec<OverlayPeerId>>(
                            profiles::gossip::codec(),
                            &bytes,
                        ) {
                            peers.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
                            peers.dedup();
                            out.insert(shard, peers);
                        }
                    }
                }
            }
        }
        out
    }

    fn register(&self, shard: ShardId, peer: OverlayPeerId) {
        let mut cache = self.cache();
        let entry = cache.entry(shard).or_default();
        if entry.contains(&peer) {
            return;
        }
        entry.push(peer);
        entry.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
        entry.dedup();
        let snapshot = entry.clone();
        drop(cache);
        if let Ok(bytes) = codec::serialize(profiles::gossip::codec(), &snapshot) {
            let key = format!("shard:{shard}");
            let mut db = self.db();
            let _ = db.insert(&key, bytes);
        }
    }

    fn peers(&self, shard: ShardId) -> Vec<OverlayPeerId> {
        self.cache().get(&shard).cloned().unwrap_or_default()
    }

    fn snapshot(&self) -> HashMap<ShardId, Vec<OverlayPeerId>> {
        self.cache().clone()
    }

    fn db(&self) -> MutexGuard<'_, SimpleDb> {
        self.db.guard()
    }

    fn cache(&self) -> MutexGuard<'_, HashMap<ShardId, Vec<OverlayPeerId>>> {
        self.cache.guard()
    }
}

#[derive(Default)]
struct RelayMetrics {
    last_fanout: Option<usize>,
    last_candidates: Option<usize>,
    avg_score: Option<f64>,
    last_updated: Option<Instant>,
    last_selected: Option<Vec<OverlayPeerId>>,
}

#[derive(Clone, Serialize)]
pub struct FanoutStatus {
    pub min: usize,
    pub base: usize,
    pub max: usize,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub last: Option<usize>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub candidates: Option<usize>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub avg_score: Option<f64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub millis_since_update: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub selected_peers: Option<Vec<String>>,
}

#[derive(Clone, Serialize)]
pub struct ShardAffinity {
    pub shard: ShardId,
    pub peers: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct PartitionStatus {
    pub active: bool,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub marker: Option<u64>,
    pub isolated_peers: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct RelayStatus {
    pub ttl_ms: u64,
    pub dedup_capacity: usize,
    pub dedup_size: usize,
    pub fanout: FanoutStatus,
    pub shard_affinity: Vec<ShardAffinity>,
    pub partition: PartitionStatus,
}

#[derive(Clone)]
struct PeerCandidate {
    addr: SocketAddr,
    transport: Transport,
    cert: Option<Bytes>,
    score: f64,
    latency_ms: Option<f64>,
    peer_kind: CandidateKind,
    peer_id: OverlayPeerId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidateKind {
    Preferred,
    Deprioritized,
}

/// Relay provides TTL-based duplicate suppression and fanout selection.
pub struct Relay {
    recent: Mutex<LruCache<[u8; 32], Instant>>,
    settings: GossipSettings,
    shard_store: ShardStore,
    metrics: Mutex<RelayMetrics>,
}

impl Relay {
    pub fn new(config: GossipConfig) -> Self {
        Self::with_engine_factory(config, SimpleDb::open_named)
    }

    pub fn with_engine_factory<F>(config: GossipConfig, factory: F) -> Self
    where
        F: Fn(&str, &str) -> SimpleDb,
    {
        let settings: GossipSettings = config.into();
        let shard_store = ShardStore::with_factory(&settings.shard_store_path, &factory);
        let recent = Mutex::new(LruCache::new(settings.dedup_capacity));
        Self {
            recent,
            settings,
            shard_store,
            metrics: Mutex::new(RelayMetrics::default()),
        }
    }

    fn hash_msg(msg: &Message) -> [u8; 32] {
        hash(&codec::serialize(profiles::gossip::codec(), msg).unwrap_or_default()).into()
    }

    pub fn should_process_at(&self, msg: &Message, now: Instant) -> bool {
        let mut guard = self.recent.guard();
        let dropped = Self::clean_expired_locked(&mut guard, self.settings.ttl, now);
        #[cfg(feature = "telemetry")]
        if dropped > 0 {
            GOSSIP_TTL_DROP_TOTAL.inc_by(dropped as u64);
        }
        #[cfg(not(feature = "telemetry"))]
        let _ = dropped;
        let h = Self::hash_msg(msg);
        if guard.peek(&h).is_some() {
            #[cfg(feature = "telemetry")]
            {
                GOSSIP_DUPLICATE_TOTAL.inc();
                with_metric_handle(
                    "gossip_peer_failure_total",
                    ["duplicate"],
                    GOSSIP_PEER_FAILURE_TOTAL.ensure_handle_for_label_values(&["duplicate"]),
                    |handle| handle.inc(),
                );
            }
            return false;
        }
        guard.put(h, now);
        true
    }

    /// Returns true if the message has not been seen recently.
    pub fn should_process(&self, msg: &Message) -> bool {
        self.should_process_at(msg, Instant::now())
    }

    fn compute_score(
        &self,
        metrics: Option<&PeerMetrics>,
        latency_ms: Option<f64>,
    ) -> (f64, CandidateKind) {
        let reputation = metrics
            .map(|m| m.reputation.score)
            .unwrap_or(self.settings.reputation_weight);
        let latency_ms = latency_ms.unwrap_or(self.settings.latency_baseline_ms);
        let latency_score = 1.0 / (1.0 + (latency_ms / self.settings.latency_baseline_ms).max(0.0));
        let success = (reputation * self.settings.reputation_weight
            + latency_score * self.settings.latency_weight)
            / (self.settings.reputation_weight + self.settings.latency_weight);
        let failures = metrics.map_or(0.0, |m| {
            let drops: u64 = m.drops.values().copied().sum();
            let handshake_fail: u64 = m.handshake_fail.values().copied().sum();
            let denom = (m.requests + m.handshake_success + 1) as f64;
            (drops + handshake_fail) as f64 / denom
        });
        let mut score = (success - failures * self.settings.failure_penalty).max(0.0);
        if score.is_nan() || !score.is_finite() {
            score = success;
        }
        let kind = if score < self.settings.low_score_cutoff {
            CandidateKind::Deprioritized
        } else {
            CandidateKind::Preferred
        };
        (score, kind)
    }

    fn gather_candidates(
        &self,
        peers: &[(SocketAddr, Transport, Option<Bytes>)],
    ) -> Vec<PeerCandidate> {
        let mut candidates = Vec::with_capacity(peers.len());
        let mut skipped_partition = 0usize;
        let stats = peer_stats_map(None, None)
            .into_iter()
            .filter_map(|(id, metrics)| {
                overlay_peer_from_base58(&id)
                    .ok()
                    .map(|peer| (peer, metrics.clone()))
                    .or_else(|| {
                        crypto_suite::hex::decode(&id)
                            .ok()
                            .and_then(|bytes| overlay_peer_from_bytes(&bytes).ok())
                            .map(|peer| (peer, metrics.clone()))
                    })
            })
            .collect::<HashMap<OverlayPeerId, PeerMetrics>>();
        for (addr, transport, cert) in peers.iter() {
            let peer_id = pk_from_addr(addr).and_then(|pk| overlay_peer_from_bytes(&pk).ok());
            let Some(peer_id) = peer_id else {
                skipped_partition += 1;
                continue;
            };
            if PARTITION_WATCH.is_isolated(&peer_id) {
                skipped_partition += 1;
                continue;
            }
            let metrics = stats.get(&peer_id);
            let latency = range_boost::peer_latency(addr)
                .map(|l| l as f64)
                .or_else(|| metrics.map(|m| m.last_handshake_ms as f64));
            let (score, kind) = self.compute_score(metrics, latency);
            candidates.push(PeerCandidate {
                addr: *addr,
                transport: *transport,
                cert: cert.clone(),
                score,
                latency_ms: latency,
                peer_kind: kind,
                peer_id: peer_id.clone(),
            });
        }
        #[cfg(feature = "telemetry")]
        if skipped_partition > 0 {
            with_metric_handle(
                "gossip_peer_failure_total",
                ["partition"],
                GOSSIP_PEER_FAILURE_TOTAL.ensure_handle_for_label_values(&["partition"]),
                |handle| handle.inc_by(skipped_partition as u64),
            );
        }
        #[cfg(not(feature = "telemetry"))]
        let _ = skipped_partition;
        candidates
    }

    fn adaptive_selection(
        &self,
        candidates: Vec<PeerCandidate>,
        fanout_all: bool,
    ) -> (Vec<PeerCandidate>, usize, f64) {
        if candidates.is_empty() {
            return (Vec::new(), 0, 0.0);
        }
        let total = candidates.len();
        let base = self
            .settings
            .base_fanout
            .min(total)
            .max(self.settings.min_fanout.min(total));
        let max = self.settings.max_fanout.min(total);
        let mut preferred: Vec<PeerCandidate> = candidates
            .iter()
            .cloned()
            .filter(|c| c.peer_kind == CandidateKind::Preferred)
            .collect();
        let mut deprioritized: Vec<PeerCandidate> = candidates
            .into_iter()
            .filter(|c| c.peer_kind == CandidateKind::Deprioritized)
            .collect();
        let avg_score = {
            let sum: f64 = preferred
                .iter()
                .chain(deprioritized.iter())
                .map(|c| c.score)
                .sum();
            sum / total as f64
        };
        let mut rng = thread_rng();
        preferred.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        deprioritized.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        let mut fanout = if fanout_all { total } else { base };
        if !fanout_all {
            if avg_score < 0.8 {
                let scale = avg_score.clamp(0.4, 1.0);
                let scaled = ((fanout as f64) * scale).ceil() as usize;
                fanout = scaled.max(self.settings.min_fanout.min(total));
            } else if avg_score > 1.2 {
                let scale = avg_score.min((self.settings.max_fanout as f64) / fanout.max(1) as f64);
                let scaled = ((fanout as f64) * scale).round() as usize;
                fanout = scaled.max(self.settings.min_fanout.min(total));
            }
        }
        fanout = fanout.min(max).max(if total > 0 { 1 } else { 0 });
        let mut selected: Vec<PeerCandidate> = preferred
            .into_iter()
            .chain(deprioritized.clone().into_iter())
            .take(fanout)
            .collect();
        if !fanout_all {
            selected.shuffle(&mut rng);
        }
        #[cfg(feature = "telemetry")]
        {
            let used_deprioritized = selected
                .iter()
                .filter(|c| c.peer_kind == CandidateKind::Deprioritized)
                .count();
            let skipped_low = deprioritized.len().saturating_sub(used_deprioritized);
            if skipped_low > 0 {
                with_metric_handle(
                    "gossip_peer_failure_total",
                    ["low_score"],
                    GOSSIP_PEER_FAILURE_TOTAL.ensure_handle_for_label_values(&["low_score"]),
                    |handle| handle.inc_by(skipped_low as u64),
                );
            }
        }
        #[cfg(not(feature = "telemetry"))]
        let _ = deprioritized;
        (selected, total, avg_score)
    }

    fn update_metrics(&self, selected: &[OverlayPeerId], candidates: usize, avg_score: f64) {
        let mut guard = self.metrics.guard();
        let fanout = selected.len();
        guard.last_fanout = Some(fanout);
        guard.last_candidates = Some(candidates);
        guard.avg_score = Some(avg_score);
        guard.last_updated = Some(Instant::now());
        guard.last_selected = if selected.is_empty() {
            None
        } else {
            Some(selected.to_vec())
        };
        #[cfg(feature = "telemetry")]
        {
            GOSSIP_FANOUT_GAUGE.set(fanout as i64);
        }
    }

    /// Broadcast a message to a random subset of peers using default sender.
    pub fn broadcast(&self, msg: &Message, peers: &[(SocketAddr, Transport, Option<Bytes>)]) {
        let serialized = codec::serialize(profiles::gossip::codec(), msg).unwrap_or_default();
        let large = serialized.len() > 1024;
        self.broadcast_with(msg, peers, |(addr, transport, cert), m| {
            if large {
                if let Some(c) = cert {
                    let _ = send_quic_msg(addr, c, m);
                } else {
                    let _ = send_msg(addr, m);
                }
            } else {
                match transport {
                    Transport::Tcp => {
                        let _ = send_msg(addr, m);
                    }
                    Transport::Quic => {
                        if let Some(c) = cert {
                            let _ = send_quic_msg(addr, c, m);
                        }
                    }
                }
            }
        });
    }

    /// Broadcast a message to peers belonging to a specific shard.
    pub fn register_peer(&self, shard: ShardId, peer: OverlayPeerId) {
        self.shard_store.register(shard, peer);
    }

    pub fn broadcast_shard(
        &self,
        shard: ShardId,
        msg: &Message,
        peers: &HashMap<OverlayPeerId, (SocketAddr, Transport, Option<Bytes>)>,
    ) {
        let ids = self.shard_store.peers(shard);
        let targets: Vec<(SocketAddr, Transport, Option<Bytes>)> = if ids.is_empty() {
            peers.values().cloned().collect()
        } else {
            ids.iter().filter_map(|id| peers.get(id).cloned()).collect()
        };
        self.broadcast(msg, &targets);
    }

    /// Broadcast using a custom send function (primarily for testing).
    pub fn broadcast_with<F>(
        &self,
        msg: &Message,
        peers: &[(SocketAddr, Transport, Option<Bytes>)],
        mut send: F,
    ) where
        F: FnMut((SocketAddr, Transport, Option<&Bytes>), &Message),
    {
        if !self.should_process(msg) {
            return;
        }
        let fanout_all = std::env::var("TB_GOSSIP_FANOUT")
            .map(|v| v == "all")
            .unwrap_or(false);
        let candidates = self.gather_candidates(peers);
        let (mut selected, candidate_len, avg_score) =
            self.adaptive_selection(candidates, fanout_all);
        let selected_ids: Vec<OverlayPeerId> = selected.iter().map(|c| c.peer_id.clone()).collect();
        self.update_metrics(&selected_ids, candidate_len, avg_score);
        if selected.is_empty() {
            return;
        }
        let partition_marker = PARTITION_WATCH.current_marker();
        let mut marked = msg.clone();
        marked.partition = partition_marker;
        for candidate in selected.drain(..) {
            #[cfg(feature = "telemetry")]
            if let Some(latency) = candidate.latency_ms {
                GOSSIP_LATENCY_BUCKETS.observe(latency / 1_000.0);
            }
            #[cfg(not(feature = "telemetry"))]
            let _ = candidate.latency_ms;
            let cert = candidate.cert.as_ref();
            send((candidate.addr, candidate.transport, cert), &marked);
        }
    }

    fn clean_expired_locked(
        cache: &mut LruCache<[u8; 32], Instant>,
        ttl: Duration,
        now: Instant,
    ) -> usize {
        let mut dropped = 0;
        while let Some(ts) = cache.peek_lru().map(|(_, ts)| *ts) {
            if now.saturating_duration_since(ts) < ttl {
                break;
            }
            cache.pop_lru();
            dropped += 1;
        }
        dropped
    }

    pub fn status(&self) -> RelayStatus {
        let dedup_size = self.recent.guard().len();
        let fanout = {
            let guard = self.metrics.guard();
            FanoutStatus {
                min: self.settings.min_fanout,
                base: self.settings.base_fanout,
                max: self.settings.max_fanout,
                last: guard.last_fanout,
                candidates: guard.last_candidates,
                avg_score: guard.avg_score,
                millis_since_update: guard
                    .last_updated
                    .map(|inst| inst.elapsed().as_millis() as u64),
                selected_peers: guard.last_selected.as_ref().map(|peers| {
                    peers
                        .iter()
                        .map(|peer| overlay_peer_to_base58(peer))
                        .collect()
                }),
            }
        };
        let shard_affinity = self
            .shard_store
            .snapshot()
            .into_iter()
            .map(|(shard, peers)| ShardAffinity {
                shard,
                peers: peers
                    .into_iter()
                    .map(|peer| overlay_peer_to_base58(&peer))
                    .collect(),
            })
            .collect();
        let partition = PartitionStatus {
            active: PARTITION_WATCH.is_partitioned(),
            marker: PARTITION_WATCH.current_marker(),
            isolated_peers: PARTITION_WATCH
                .isolated_peers()
                .into_iter()
                .map(|peer| overlay_peer_to_base58(&peer))
                .collect(),
        };
        RelayStatus {
            ttl_ms: self.settings.ttl.as_millis() as u64,
            dedup_capacity: self.settings.dedup_capacity.get(),
            dedup_size,
            fanout,
            shard_affinity,
            partition,
        }
    }
}

impl Default for Relay {
    fn default() -> Self {
        Self::new(config::current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Payload;
    use crypto_suite::signatures::ed25519::SigningKey;

    fn test_settings() -> GossipSettings {
        GossipSettings::from(GossipConfig {
            ttl_ms: 10,
            dedup_capacity: 1024,
            min_fanout: 2,
            base_fanout: 4,
            max_fanout: 8,
            failure_penalty: 1.5,
            latency_weight: 0.6,
            reputation_weight: 1.0,
            latency_baseline_ms: 40,
            low_score_cutoff: 0.55,
            shard_store_path: String::new(),
        })
    }

    fn relay_for_tests() -> Relay {
        let settings = test_settings();
        let shard_store = ShardStore::temporary();
        Relay {
            recent: Mutex::new(LruCache::new(settings.dedup_capacity)),
            settings,
            shard_store,
            metrics: Mutex::new(RelayMetrics::default()),
        }
    }

    #[test]
    fn relay_dedup_respects_ttl() {
        let relay = relay_for_tests();
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk).expect("sign hello");
        let now = Instant::now();
        assert!(relay.should_process_at(&msg, now));
        assert!(!relay.should_process_at(&msg, now));
        std::thread::sleep(Duration::from_millis(20));
        assert!(relay.should_process(&msg));
    }

    #[test]
    fn relay_respects_fanout_bounds() {
        let relay = relay_for_tests();
        let sk = SigningKey::from_bytes(&[2u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk).expect("sign hello");
        let peers: Vec<(SocketAddr, Transport, Option<Bytes>)> = (0..16)
            .map(|i| {
                let port = match u16::try_from(10000 + i) {
                    Ok(port) => port,
                    Err(_) => panic!("test port out of range"),
                };
                (
                    SocketAddr::from(([127, 0, 0, 1], port)),
                    Transport::Tcp,
                    None,
                )
            })
            .collect();
        for (idx, (addr, _, _)) in peers.iter().enumerate() {
            let peer = match crate::net::overlay_peer_from_bytes(&[(idx as u8) + 1; 32]) {
                Ok(peer) => peer,
                Err(err) => panic!("peer id decode failed: {err}"),
            };
            crate::net::peer::inject_addr_mapping_for_tests(*addr, peer);
        }
        let mut delivered = 0usize;
        relay.broadcast_with(&msg, &peers, |_, _| delivered += 1);
        assert!(delivered >= relay.settings.min_fanout);
        assert!(delivered <= relay.settings.max_fanout);
    }

    #[test]
    fn relay_shuffle_prevents_bias() {
        let relay = relay_for_tests();
        let sk = SigningKey::from_bytes(&[3u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk).expect("sign hello");
        let peers: Vec<(SocketAddr, Transport, Option<Bytes>)> = (0..8)
            .map(|i| {
                let port = match u16::try_from(12000 + i) {
                    Ok(port) => port,
                    Err(_) => panic!("test port out of range"),
                };
                (
                    SocketAddr::from(([127, 0, 0, 1], port)),
                    Transport::Tcp,
                    None,
                )
            })
            .collect();
        for (idx, (addr, _, _)) in peers.iter().enumerate() {
            let peer = match crate::net::overlay_peer_from_bytes(&[(idx as u8) + 1; 32]) {
                Ok(peer) => peer,
                Err(err) => panic!("peer id decode failed: {err}"),
            };
            crate::net::peer::inject_addr_mapping_for_tests(*addr, peer);
        }
        let mut first_hits = HashMap::new();
        for _ in 0..20 {
            let mut calls = Vec::new();
            relay.broadcast_with(&msg, &peers, |peer, _| calls.push(peer.0));
            if let Some(addr) = calls.first() {
                *first_hits.entry(*addr).or_insert(0usize) += 1;
            }
            relay.recent.guard().clear();
        }
        assert!(first_hits.len() > 1);
    }

    #[test]
    fn relay_status_surfaces_selected_peers_as_base58() {
        let relay = relay_for_tests();
        let peer = match crate::net::overlay_peer_from_bytes(&[9u8; 32]) {
            Ok(peer) => peer,
            Err(err) => panic!("overlay peer decode failed: {err}"),
        };
        relay.update_metrics(&[peer.clone()], 3, 1.4);

        let status = relay.status();
        let selected = match status.fanout.selected_peers {
            Some(peers) => peers,
            None => panic!("selected peers missing"),
        };
        assert_eq!(selected, vec![crate::net::overlay_peer_to_base58(&peer)]);
    }
}
