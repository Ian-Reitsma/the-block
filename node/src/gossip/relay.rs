use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use blake3::hash;
use rand::seq::SliceRandom;

use crate::net::Message;
use crate::net::send_msg;
#[cfg(feature = "telemetry")]
use crate::telemetry::{GOSSIP_DUPLICATE_TOTAL, GOSSIP_FANOUT_GAUGE};

/// Relay provides TTL-based duplicate suppression and fanout selection.
pub struct Relay {
    recent: Mutex<HashMap<[u8; 32], Instant>>,
    ttl: Duration,
}

impl Relay {
    pub fn new(ttl: Duration) -> Self {
        Self {
            recent: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    fn hash_msg(msg: &Message) -> [u8; 32] {
        hash(&bincode::serialize(msg).unwrap_or_default()).into()
    }

    /// Returns true if the message has not been seen recently.
    pub fn should_process(&self, msg: &Message) -> bool {
        let h = Self::hash_msg(msg);
        let mut guard = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        guard.retain(|_, t| now.duration_since(*t) < self.ttl);
        if guard.contains_key(&h) {
            #[cfg(feature = "telemetry")]
            GOSSIP_DUPLICATE_TOTAL.inc();
            false
        } else {
            guard.insert(h, now);
            true
        }
    }

    fn compute_fanout(num_peers: usize) -> usize {
        ((num_peers as f64).sqrt().ceil() as usize).min(16)
    }

    /// Broadcast a message to a random subset of peers using default sender.
    pub fn broadcast(&self, msg: &Message, peers: &[SocketAddr]) {
        self.broadcast_with(msg, peers, |addr, m| {
            let _ = send_msg(addr, m);
        });
    }

    /// Broadcast using a custom send function (primarily for testing).
    pub fn broadcast_with<F>(&self, msg: &Message, peers: &[SocketAddr], mut send: F)
    where
        F: FnMut(SocketAddr, &Message),
    {
        if !self.should_process(msg) {
            return;
        }
        let fanout = Self::compute_fanout(peers.len()).min(peers.len().max(1));
        #[cfg(feature = "telemetry")]
        GOSSIP_FANOUT_GAUGE.set(fanout as i64);
        let mut rng = rand::thread_rng();
        let mut list = peers.to_vec();
        list.shuffle(&mut rng);
        for addr in list.into_iter().take(fanout) {
            send(addr, msg);
        }
    }
}

impl Default for Relay {
    fn default() -> Self {
        Self::new(Duration::from_secs(2))
    }
}
