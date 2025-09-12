use bytes::{BufMut, BytesMut};
use parking_lot::Mutex;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;

use blake3::Hasher;
use once_cell::sync::Lazy;

use crate::net::message::BlobChunk;
use crate::net::peer::is_throttled_addr;
use crate::net::peer::ReputationUpdate;
use crate::net::{record_ip_drop, send_msg, send_quic_msg, Message};
use crate::p2p::handshake::Transport;
use ed25519_dalek::SigningKey;

/// Deterministic fanout tree inspired by Turbine gossip.
pub fn broadcast(msg: &Message, peers: &[(SocketAddr, Transport, Option<Vec<u8>>)]) {
    broadcast_with(msg, peers, |(addr, transport, cert), m| match transport {
        Transport::Tcp => {
            let _ = send_msg(addr, m);
        }
        Transport::Quic => {
            if let Some(c) = cert {
                let _ = send_quic_msg(addr, &c, m);
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
    peers: &[(SocketAddr, Transport, Option<Vec<u8>>)],
) {
    let msg = Message::new(crate::net::message::Payload::BlobChunk(chunk.clone()), sk);
    broadcast(&msg, peers);
}

/// Broadcast reputation gossip entries.
pub fn broadcast_reputation(
    entries: &[ReputationUpdate],
    sk: &SigningKey,
    peers: &[(SocketAddr, Transport, Option<Vec<u8>>)],
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
    peers: &[(SocketAddr, Transport, Option<Vec<u8>>)],
    mut send: F,
) where
    F: FnMut((SocketAddr, Transport, Option<&[u8]>), &Message),
{
    static SEEN: Lazy<Mutex<(HashSet<[u8; 32]>, VecDeque<[u8; 32]>)>> =
        Lazy::new(|| Mutex::new((HashSet::new(), VecDeque::new())));
    const MAX_SEEN: usize = 1024;
    let hash = {
        let size = bincode::serialized_size(msg).unwrap_or(0) as usize;
        let mut buf = BytesMut::with_capacity(size);
        bincode::serialize_into(&mut buf.writer(), msg).unwrap_or_default();
        let mut h = Hasher::new();
        h.update(&buf);
        *h.finalize().as_bytes()
    };
    let mut guard = SEEN.lock();
    if guard.0.contains(&hash) {
        for p in peers {
            record_ip_drop(p);
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
            let cert = peers[idx].2.as_deref();
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
