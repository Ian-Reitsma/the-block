use crypto_suite::hashing::blake3::Hasher;
use ledger::address::ShardId;
use lru::LruCache;
use std::collections::VecDeque;

#[cfg(feature = "telemetry")]
use crate::telemetry::INTER_SHARD_REPLAY_EVICT_TOTAL;

/// Simple in-memory inter-shard message queue with replay protection and
/// commitment proofs.
pub struct MessageQueue {
    seen: LruCache<[u8; 32], ()>,
    queue: VecDeque<(ShardId, Vec<u8>)>,
    evictions: u64,
}

impl MessageQueue {
    pub fn new(max_seen: usize) -> Self {
        Self {
            seen: LruCache::new(max_seen.try_into().unwrap()),
            queue: VecDeque::new(),
            evictions: 0,
        }
    }

    pub fn eviction_count(&self) -> u64 {
        self.evictions
    }

    pub fn seen_len(&self) -> usize {
        self.seen.len()
    }

    fn capacity(&self) -> usize {
        self.seen.cap().into()
    }

    /// Compute the hash used as a Merkle tree leaf for `(dest, msg)`.
    fn leaf_hash(dest: ShardId, msg: &[u8]) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&dest.to_le_bytes());
        h.update(msg);
        *h.finalize().as_bytes()
    }

    /// Recompute the current Merkle root of the queue.
    pub fn root(&self) -> [u8; 32] {
        let leaves: Vec<[u8; 32]> = self
            .queue
            .iter()
            .map(|(d, m)| Self::leaf_hash(*d, m))
            .collect();
        merkle_root(&leaves)
    }

    /// Enqueue a message for a destination shard.
    pub fn enqueue(&mut self, dest: ShardId, msg: Vec<u8>) {
        let leaf = Self::leaf_hash(dest, &msg);
        if !self.seen.contains(&leaf) {
            if self.seen.len() == self.capacity() {
                self.evictions += 1;
                #[cfg(feature = "telemetry")]
                {
                    INTER_SHARD_REPLAY_EVICT_TOTAL.inc();
                }
            }
            self.seen.put(leaf, ());
            self.queue.push_back((dest, msg));
        }
    }

    /// Pop the next pending message returning a Merkle proof of inclusion.
    pub fn dequeue(&mut self) -> Option<(ShardId, Vec<u8>, Vec<[u8; 32]>)> {
        if self.queue.is_empty() {
            return None;
        }
        let leaves: Vec<[u8; 32]> = self
            .queue
            .iter()
            .map(|(d, m)| Self::leaf_hash(*d, m))
            .collect();
        let proof = merkle_proof(&leaves, 0);
        let (dest, msg) = self.queue.pop_front()?;
        self.seen.pop(&Self::leaf_hash(dest, &msg));
        Some((dest, msg, proof))
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Compute the Merkle root of the provided leaf hashes.
fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::new();
        for i in (0..level.len()).step_by(2) {
            let left = level[i];
            let right = if i + 1 < level.len() {
                level[i + 1]
            } else {
                left
            };
            let mut h = Hasher::new();
            h.update(&left);
            h.update(&right);
            next.push(*h.finalize().as_bytes());
        }
        level = next;
    }
    level[0]
}

/// Generate the Merkle proof for the leaf at `idx`.
fn merkle_proof(leaves: &[[u8; 32]], mut idx: usize) -> Vec<[u8; 32]> {
    let mut path = Vec::new();
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let sibling = if idx % 2 == 0 {
            if idx + 1 < level.len() {
                level[idx + 1]
            } else {
                level[idx]
            }
        } else {
            level[idx - 1]
        };
        path.push(sibling);
        let mut next = Vec::new();
        for i in (0..level.len()).step_by(2) {
            let left = level[i];
            let right = if i + 1 < level.len() {
                level[i + 1]
            } else {
                left
            };
            let mut h = Hasher::new();
            h.update(&left);
            h.update(&right);
            next.push(*h.finalize().as_bytes());
        }
        level = next;
        idx /= 2;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_proof(mut leaf: [u8; 32], mut idx: usize, path: &[[u8; 32]]) -> [u8; 32] {
        for sibling in path {
            let mut h = Hasher::new();
            if idx % 2 == 0 {
                h.update(&leaf);
                h.update(sibling);
            } else {
                h.update(sibling);
                h.update(&leaf);
            }
            leaf = *h.finalize().as_bytes();
            idx /= 2;
        }
        leaf
    }

    #[test]
    fn dequeue_provides_valid_proof() {
        let mut q = MessageQueue::default();
        q.enqueue(1, b"hello".to_vec());
        q.enqueue(2, b"world".to_vec());
        let root = q.root();
        let (dest, msg, proof) = q.dequeue().unwrap();
        let leaf = MessageQueue::leaf_hash(dest, &msg);
        assert_eq!(verify_proof(leaf, 0, &proof), root);
    }

    #[test]
    fn lru_evicts_old_entries() {
        let mut q = MessageQueue::new(2);
        q.enqueue(1, b"a".to_vec());
        q.enqueue(1, b"b".to_vec());
        q.enqueue(1, b"c".to_vec());
        assert_eq!(q.seen_len(), 2);
        assert_eq!(q.eviction_count(), 1);
    }
}
