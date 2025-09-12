use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use blake3::hash;
use rand::seq::SliceRandom;

use crate::net::{send_msg, send_quic_msg, Message};
use crate::p2p::handshake::Transport;
use crate::range_boost;
#[cfg(feature = "telemetry")]
use crate::telemetry::{GOSSIP_DUPLICATE_TOTAL, GOSSIP_FANOUT_GAUGE, GOSSIP_TTL_DROP_TOTAL};

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
        let before = guard.len();
        guard.retain(|_, t| now.duration_since(*t) < self.ttl);
        let dropped = before - guard.len();
        #[cfg(feature = "telemetry")]
        if dropped > 0 {
            GOSSIP_TTL_DROP_TOTAL.inc_by(dropped as u64);
        }
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
    pub fn broadcast(&self, msg: &Message, peers: &[(SocketAddr, Transport, Option<Vec<u8>>)]) {
        self.broadcast_with(msg, peers, |(addr, transport, cert), m| match transport {
            Transport::Tcp => {
                let _ = send_msg(addr, m);
            }
            Transport::Quic => {
                if let Some(c) = cert {
                    let _ = send_quic_msg(addr, &c, m);
                }
            }
        });
    }

    /// Broadcast using a custom send function (primarily for testing).
    pub fn broadcast_with<F>(
        &self,
        msg: &Message,
        peers: &[(SocketAddr, Transport, Option<Vec<u8>>)],
        mut send: F,
    ) where
        F: FnMut((SocketAddr, Transport, Option<&[u8]>), &Message),
    {
        if !self.should_process(msg) {
            return;
        }
        let fanout_all = std::env::var("TB_GOSSIP_FANOUT")
            .map(|v| v == "all")
            .unwrap_or(false);
        let mut list = peers.to_vec();
        list.sort_by_key(|(addr, _, _)| range_boost::peer_latency(addr).unwrap_or(u128::MAX));
        let fanout = if fanout_all {
            list.len()
        } else {
            Self::compute_fanout(list.len()).min(list.len().max(1))
        };
        #[cfg(feature = "telemetry")]
        GOSSIP_FANOUT_GAUGE.set(fanout as i64);
        if !fanout_all {
            list.truncate(fanout);
            let mut rng = rand::thread_rng();
            list.shuffle(&mut rng);
        }
        for peer in list.into_iter().take(fanout) {
            let cert = peer.2.as_deref();
            send((peer.0, peer.1, cert), msg);
        }
    }
}

impl Default for Relay {
    fn default() -> Self {
        Self::new(Duration::from_secs(2))
    }
}
