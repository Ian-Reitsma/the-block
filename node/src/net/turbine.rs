use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Mutex;

use concurrency::{Lazy, MutexExt};
use crypto_suite::hashing::blake3::Hasher;

use crate::net::message::BlobChunk;
use crate::net::peer::is_throttled_addr;
use crate::net::peer::ReputationUpdate;
use crate::net::{record_ip_drop, send_msg, send_quic_msg, Message};
use crate::p2p::handshake::Transport;
use concurrency::Bytes;
use crypto_suite::signatures::ed25519::SigningKey;

/// Deterministic fanout tree inspired by Turbine gossip.
pub fn broadcast(msg: &Message, peers: &[(SocketAddr, Transport, Option<Bytes>)]) {
    broadcast_with(msg, peers, |(addr, transport, cert), m| match transport {
        Transport::Tcp => {
            let _ = send_msg(addr, m);
        }
        Transport::Quic => {
            if let Some(c) = cert {
                let _ = send_quic_msg(addr, c, m);
            } else {
                let _ = send_msg(addr, m);
            }
        }
    });
}

/// Broadcast a blob shard using Turbine fan-out, signing with `sk`.
pub fn broadcast_chunk(
    chunk: &BlobChunk,
    sk: &SigningKey,
    peers: &[(SocketAddr, Transport, Option<Bytes>)],
) {
    let msg = Message::new(crate::net::message::Payload::BlobChunk(chunk.clone()), sk);
    broadcast(&msg, peers);
}

/// Broadcast reputation gossip entries.
pub fn broadcast_reputation(
    entries: &[ReputationUpdate],
    sk: &SigningKey,
    peers: &[(SocketAddr, Transport, Option<Bytes>)],
) {
    let msg = Message::new(
        crate::net::message::Payload::Reputation(entries.to_vec()),
        sk,
    );
    broadcast(&msg, peers);
}

/// Broadcast with a custom send function, useful for tests.
pub fn broadcast_with<F>(
    msg: &Message,
    peers: &[(SocketAddr, Transport, Option<Bytes>)],
    mut send: F,
) where
    F: FnMut((SocketAddr, Transport, Option<&Bytes>), &Message),
{
    static SEEN: Lazy<Mutex<(HashSet<[u8; 32]>, VecDeque<[u8; 32]>)>> =
        Lazy::new(|| Mutex::new((HashSet::new(), VecDeque::new())));
    const MAX_SEEN: usize = 1024;
    let hash = {
        let size = bincode::serialized_size(msg).unwrap_or(0) as usize;
        let mut buf = Vec::with_capacity(size);
        bincode::serialize_into(&mut buf, msg).unwrap_or_default();
        let mut h = Hasher::new();
        h.update(&buf);
        *h.finalize().as_bytes()
    };
    let mut guard = SEEN.guard();
    if guard.0.contains(&hash) {
        for p in peers {
            record_ip_drop(&p.0);
        }
        return;
    }
    guard.0.insert(hash);
    guard.1.push_back(hash);
    if guard.0.len() > MAX_SEEN {
        if let Some(old) = guard.1.pop_front() {
            guard.0.remove(&old);
        }
    }
    drop(guard);

    if peers.is_empty() {
        return;
    }
    let fanout = compute_fanout(peers.len());
    let mut queue = vec![0usize];
    let mut seen = vec![false; peers.len()];
    while let Some(idx) = queue.pop() {
        if idx >= peers.len() || seen[idx] {
            continue;
        }
        seen[idx] = true;
        if !is_throttled_addr(&peers[idx].0) {
            let cert = peers[idx].2.as_ref();
            send((peers[idx].0, peers[idx].1, cert), msg);
        }
        for i in 1..=fanout {
            queue.push(idx * fanout + i);
        }
    }
}

fn compute_fanout(num_peers: usize) -> usize {
    ((num_peers as f64).sqrt().ceil() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::message::Payload;
    use crate::net::peer::take_recorded_drops;
    use crate::p2p::handshake::{Hello, SUPPORTED_VERSION};

    fn sample_message() -> Message {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: SUPPORTED_VERSION,
            feature_bits: 0,
            agent: "test".into(),
            nonce: 42,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        Message::new(Payload::Handshake(hello), &sk)
    }

    #[test]
    fn duplicate_broadcast_records_drop() {
        let peers = vec![("127.0.0.1:9000".parse().unwrap(), Transport::Tcp, None)];
        let _ = take_recorded_drops();

        let msg = sample_message();
        broadcast(&msg, &peers);
        broadcast(&msg, &peers);

        let drops = take_recorded_drops();
        assert_eq!(drops, vec![peers[0].0]);
    }
}
