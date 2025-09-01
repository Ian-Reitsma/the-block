use super::{load_net_key, send_msg, PROTOCOL_VERSION};
#[cfg(feature = "telemetry")]
use crate::consensus::observer;
use crate::net::message::{Message, Payload};
use crate::Blockchain;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use once_cell::sync::Lazy;
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ban_store;

/// Thread-safe peer set used by the gossip layer.
#[derive(Clone, Default)]
pub struct PeerSet {
    addrs: Arc<Mutex<HashSet<SocketAddr>>>,
    authorized: Arc<Mutex<HashSet<[u8; 32]>>>,
    states: Arc<Mutex<HashMap<[u8; 32], PeerState>>>,
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
        Self {
            addrs: Arc::new(Mutex::new(set)),
            authorized: Arc::new(Mutex::new(HashSet::new())),
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a peer to the set.
    pub fn add(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.insert(addr);
            persist_peers(&guard);
        }
    }

    /// Remove a peer from the set.
    pub fn remove(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.remove(&addr);
            persist_peers(&guard);
        }
    }

    /// Clear all peers from the set.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.addrs.lock() {
            guard.clear();
            persist_peers(&guard);
        }
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.addrs
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Return a randomized list of peers for bootstrapping.
    pub fn bootstrap(&self) -> Vec<SocketAddr> {
        let mut peers = self.list();
        let seed = std::env::var("TB_PEER_SEED")
            .ok()
            .and_then(|v| v.parse().ok());
        let mut rng: StdRng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
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
        if entry.count > *P2P_MAX_PER_SEC {
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

        if let Err(code) = self.check_rate(&msg.pubkey) {
            telemetry_peer_error(code);
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
                if hs.protocol_version != PROTOCOL_VERSION {
                    telemetry_peer_error(PeerErrorCode::HandshakeVersion);
                    #[cfg(feature = "telemetry")]
                    {
                        crate::telemetry::PEER_REJECTED_TOTAL
                            .with_label_values(&["protocol"])
                            .inc();
                        crate::telemetry::PEER_HANDSHAKE_FAILURE_TOTAL
                            .with_label_values(&["protocol"])
                            .inc();
                    }
                    return;
                }
                if (hs.features & crate::net::REQUIRED_FEATURES) != crate::net::REQUIRED_FEATURES {
                    telemetry_peer_error(PeerErrorCode::HandshakeFeature);
                    #[cfg(feature = "telemetry")]
                    {
                        crate::telemetry::PEER_HANDSHAKE_FAILURE_TOTAL
                            .with_label_values(&["feature"])
                            .inc();
                    }
                    return;
                }
                self.authorize(msg.pubkey);
                if let Some(peer_addr) = addr {
                    self.add(peer_addr);
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
        }
    }
}

struct PeerState {
    count: u32,
    last: Instant,
    banned_until: Option<Instant>,
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

fn persist_peers(set: &HashSet<SocketAddr>) {
    let path = peer_db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut addrs: Vec<String> = set.iter().map(|a| a.to_string()).collect();
    addrs.sort();
    let _ = fs::write(path, addrs.join("\n"));
}

static P2P_MAX_PER_SEC: Lazy<u32> = Lazy::new(|| {
    std::env::var("TB_P2P_MAX_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
});

static P2P_BAN_SECS: Lazy<u64> = Lazy::new(|| {
    std::env::var("TB_P2P_BAN_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
});
