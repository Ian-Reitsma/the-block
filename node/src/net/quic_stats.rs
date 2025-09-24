use hex;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::quic::{self, ConnectionStatsSnapshot};
use super::transport_quic;
use crate::p2p::handshake;

const CACHE_TTL: Duration = Duration::from_millis(500);
#[cfg(feature = "telemetry")]
const UNKNOWN_PEER_LABEL: &str = "unknown";

#[derive(Default, Clone)]
struct PeerState {
    latency_ms: Option<u64>,
    retransmits: u64,
    endpoint_reuse: u64,
    handshake_failures: u64,
    last_updated: u64,
    address: Option<SocketAddr>,
}

#[derive(Default)]
struct Cache {
    entries: Vec<super::QuicStatsEntry>,
    last_refresh: Option<Instant>,
}

#[derive(Default)]
struct Store {
    peers: HashMap<[u8; 32], PeerState>,
    cache: Cache,
}

static STORE: Lazy<RwLock<Store>> = Lazy::new(|| RwLock::new(Store::default()));

fn now() -> u64 {
    super::unix_now()
}

fn invalidate_cache(store: &mut Store) {
    store.cache.last_refresh = None;
}

pub(super) fn record_latency(peer: &[u8; 32], ms: u64) {
    let mut guard = STORE.write().unwrap();
    let entry = guard.peers.entry(*peer).or_default();
    entry.latency_ms = Some(ms);
    entry.last_updated = now();
    invalidate_cache(&mut guard);
}

pub(super) fn record_handshake_failure(peer: &[u8; 32]) {
    let mut guard = STORE.write().unwrap();
    let entry = guard.peers.entry(*peer).or_default();
    entry.handshake_failures = entry.handshake_failures.saturating_add(1);
    entry.last_updated = now();
    invalidate_cache(&mut guard);
}

pub(super) fn record_endpoint_reuse(peer: &[u8; 32]) {
    let mut guard = STORE.write().unwrap();
    let entry = guard.peers.entry(*peer).or_default();
    entry.endpoint_reuse = entry.endpoint_reuse.saturating_add(1);
    entry.last_updated = now();
    invalidate_cache(&mut guard);
}

pub(super) fn record_address(peer: &[u8; 32], addr: SocketAddr) {
    let mut guard = STORE.write().unwrap();
    let entry = guard.peers.entry(*peer).or_default();
    entry.address = Some(addr);
    entry.last_updated = now();
    invalidate_cache(&mut guard);
}

pub(super) fn snapshot() -> Vec<super::QuicStatsEntry> {
    let mut guard = STORE.write().unwrap();
    let expired = guard
        .cache
        .last_refresh
        .map_or(true, |ts| ts.elapsed() > CACHE_TTL);
    if expired {
        refresh_locked(&mut guard);
    }
    guard.cache.entries.clone()
}

fn refresh_locked(store: &mut Store) {
    for (addr, stats) in quic::connection_stats() {
        note_stats_locked(store, addr, stats);
    }
    let mut entries: Vec<_> = store
        .peers
        .iter()
        .map(|(peer, state)| {
            let provider = handshake::peer_provider(peer);
            let fingerprint = if let Some(ref id) = provider {
                super::current_peer_fingerprint_for_provider(peer, Some(id.as_str()))
            } else {
                super::current_peer_fingerprint(peer)
            }
            .map(|fp| hex::encode(fp));
            super::QuicStatsEntry {
                peer_id: hex::encode(peer),
                address: state.address.map(|a| a.to_string()),
                latency_ms: state.latency_ms,
                fingerprint,
                provider,
                retransmits: state.retransmits,
                endpoint_reuse: state.endpoint_reuse,
                handshake_failures: state.handshake_failures,
                last_updated: state.last_updated,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    store.cache.entries = entries;
    store.cache.last_refresh = Some(Instant::now());
}

fn note_stats_locked(store: &mut Store, addr: SocketAddr, stats: ConnectionStatsSnapshot) {
    if let Some(peer) = super::peer::pk_from_addr(&addr) {
        let entry = store.peers.entry(peer).or_default();
        update_retransmits(entry, stats.lost_packets);
        entry.latency_ms = Some(stats.rtt.as_millis() as u64);
        entry.address = Some(addr);
        entry.last_updated = now();
    }
}

fn update_retransmits(entry: &mut PeerState, latest: u64) {
    if latest > entry.retransmits {
        transport_quic::record_retransmit(latest - entry.retransmits);
    }
    entry.retransmits = latest;
}

#[cfg(feature = "telemetry")]
pub(super) fn peer_label(peer: Option<[u8; 32]>) -> String {
    peer.map(|pk| hex::encode(pk))
        .unwrap_or_else(|| UNKNOWN_PEER_LABEL.to_string())
}
