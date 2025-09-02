use std::net::SocketAddr;

use crate::net::{send_msg, Message};
use crate::net::message::BlobChunk;
use ed25519_dalek::SigningKey;

/// Deterministic fanout tree inspired by Turbine gossip.
pub fn broadcast(msg: &Message, peers: &[SocketAddr]) {
    broadcast_with(msg, peers, |addr, m| {
        let _ = send_msg(addr, m);
    });
}

/// Broadcast a blob shard using Turbine fan-out, signing with `sk`.
pub fn broadcast_chunk(chunk: &BlobChunk, sk: &SigningKey, peers: &[SocketAddr]) {
    let msg = Message::new(crate::net::message::Payload::BlobChunk(chunk.clone()), sk);
    broadcast(&msg, peers);
}

/// Broadcast with a custom send function, useful for tests.
pub fn broadcast_with<F>(msg: &Message, peers: &[SocketAddr], mut send: F)
where
    F: FnMut(SocketAddr, &Message),
{
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
        send(peers[idx], msg);
        for i in 1..=fanout {
            queue.push(idx * fanout + i);
        }
    }
}

fn compute_fanout(num_peers: usize) -> usize {
    ((num_peers as f64).sqrt().ceil() as usize).max(1)
}
