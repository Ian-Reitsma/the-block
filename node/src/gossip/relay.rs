use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use blake3::hash;
use rand::seq::SliceRandom;

use crate::net::{partition_watch::PARTITION_WATCH, send_msg, send_quic_msg, Message, Transport};
use crate::range_boost;
#[cfg(feature = "telemetry")]
use crate::telemetry::{GOSSIP_DUPLICATE_TOTAL, GOSSIP_FANOUT_GAUGE, GOSSIP_TTL_DROP_TOTAL};
use ledger::address::ShardId;

type PeerId = [u8; 32];

/// Relay provides TTL-based duplicate suppression and fanout selection.
pub struct Relay {
    recent: Mutex<HashMap<[u8; 32], Instant>>,
    ttl: Duration,
    shard_peers: Mutex<HashMap<ShardId, Vec<PeerId>>>,
}

impl Relay {
    pub fn new(ttl: Duration) -> Self {
        Self {
            recent: Mutex::new(HashMap::new()),
            ttl,
            shard_peers: Mutex::new(HashMap::new()),
        }
    }

    fn hash_msg(msg: &Message) -> [u8; 32] {
        hash(&bincode::serialize(msg).unwrap_or_default()).into()
    }

    /// Returns true if the message has not been seen recently.
    pub fn should_process(&self, msg: &Message) -> bool {
        let h = Self::hash_msg(msg);
        let mut guard = self.recent.lock();
        let now = Instant::now();
        let before = guard.len();
        guard.retain(|_, t| now.duration_since(*t) < self.ttl);
        let dropped = before - guard.len();
        #[cfg(feature = "telemetry")]
        if dropped > 0 {
            GOSSIP_TTL_DROP_TOTAL.inc_by(dropped as u64);
        }
        #[cfg(not(feature = "telemetry"))]
        let _ = dropped;
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
        let serialized = bincode::serialize(msg).unwrap_or_default();
        let large = serialized.len() > 1024;
        self.broadcast_with(msg, peers, |(addr, transport, cert), m| {
            if large {
                if let Some(c) = cert {
                    let _ = send_quic_msg(addr, &c, m);
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
                            let _ = send_quic_msg(addr, &c, m);
                        }
                    }
                }
            }
        });
    }

    /// Broadcast a message to peers belonging to a specific shard.
    pub fn register_peer(&self, shard: ShardId, peer: PeerId) {
        self.shard_peers.lock().entry(shard).or_default().push(peer);
    }

    pub fn broadcast_shard(
        &self,
        shard: ShardId,
        msg: &Message,
        peers: &HashMap<PeerId, (SocketAddr, Transport, Option<Vec<u8>>)>,
    ) {
        let ids = self.shard_peers.lock().get(&shard).cloned();
        let targets: Vec<(SocketAddr, Transport, Option<Vec<u8>>)> = if let Some(ids) = ids {
            ids.iter().filter_map(|id| peers.get(id).cloned()).collect()
        } else {
            peers.values().cloned().collect()
        };
        self.broadcast(msg, &targets);
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
        if range_boost::is_enabled() {
            list.sort_by_key(|(addr, _, _)| range_boost::peer_latency(addr).unwrap_or(u128::MAX));
        }
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
            let mut marked = msg.clone();
            marked.partition = PARTITION_WATCH.current_marker();
            send((peer.0, peer.1, cert), &marked);
        }
    }
}

impl Default for Relay {
    fn default() -> Self {
        Self::new(Duration::from_secs(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use std::net::SocketAddr;

    use crate::net::Payload;

    #[test]
    fn relay_dedup_and_fanout() {
        let relay = Relay::new(Duration::from_secs(2));
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk);
        assert!(relay.should_process(&msg));
        assert!(!relay.should_process(&msg));
        #[cfg(feature = "telemetry")]
        assert!(crate::telemetry::GOSSIP_DUPLICATE_TOTAL.get() > 0);

        let peers: Vec<(SocketAddr, Transport, Option<Vec<u8>>)> = (0..25)
            .map(|i| {
                (
                    format!("127.0.0.1:{}", 10000 + i).parse().unwrap(),
                    Transport::Tcp,
                    None,
                )
            })
            .collect();
        let msg2 = Message::new(Payload::Hello(vec![peers[0].0]), &sk);
        let expected = ((peers.len() as f64).sqrt().ceil() as usize).min(16);
        let mut delivered = 0usize;
        let mut count = 0usize;
        let loss = (expected as f64 * 0.15).ceil() as usize;
        relay.broadcast_with(&msg2, &peers, |_, _| {
            if count >= loss {
                delivered += 1;
            }
            count += 1;
        });
        assert_eq!(count, expected);
        assert!(delivered >= expected - loss);
    }

    #[test]
    fn relay_mixed_transport_fanout() {
        std::env::set_var("TB_GOSSIP_FANOUT", "all");
        let relay = Relay::default();
        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk);
        let peers = vec![
            ("127.0.0.1:10000".parse().unwrap(), Transport::Tcp, None),
            (
                "127.0.0.1:10001".parse().unwrap(),
                Transport::Quic,
                Some(vec![1, 2, 3]),
            ),
        ];
        let mut seen: Vec<(SocketAddr, Transport)> = Vec::new();
        relay.broadcast_with(&msg, &peers, |(addr, t, _), _| seen.push((addr, t)));
        assert_eq!(seen.len(), 2);
        assert!(seen
            .iter()
            .any(|(a, t)| (*a, *t) == (peers[0].0, peers[0].1)));
        assert!(seen
            .iter()
            .any(|(a, t)| (*a, *t) == (peers[1].0, peers[1].1)));
        std::env::remove_var("TB_GOSSIP_FANOUT");
    }
}
