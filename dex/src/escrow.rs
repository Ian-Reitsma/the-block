#![forbid(unsafe_code)]

use crypto_suite::hashing::{blake3, sha3::Sha3_256};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::BTreeMap;
use subtle::ConstantTimeEq;

/// Unique identifier for an escrow entry.
pub type EscrowId = u64;

/// Supported hash commitment schemes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HashAlgo {
    Blake3,
    Sha3,
}

/// A Merkle proof for a particular partial payment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentProof {
    /// Hash of the payment leaf.
    pub leaf: [u8; 32],
    /// Sibling hashes from leaf to root.
    pub path: Vec<[u8; 32]>,
    /// Commitment scheme used for hashing.
    pub algo: HashAlgo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowEntry {
    pub from: String,
    pub to: String,
    pub total: u64,
    pub released: u64,
    pub payments: Vec<u64>,
    pub root: [u8; 32],
    pub algo: HashAlgo,
}

impl Default for EscrowEntry {
    fn default() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            total: 0,
            released: 0,
            payments: Vec::new(),
            root: blake3::hash(&[]).into(),
            algo: HashAlgo::Blake3,
        }
    }
}

/// Escrow table storing entries keyed by `EscrowId`.
#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct Escrow {
    entries: BTreeMap<EscrowId, EscrowEntry>,
    next_id: EscrowId,
    total_locked: u64,
}

impl Escrow {
    pub fn lock_with_algo(
        &mut self,
        from: String,
        to: String,
        total: u64,
        algo: HashAlgo,
    ) -> EscrowId {
        let id = self.next_id;
        self.next_id += 1;
        let entry = EscrowEntry {
            from,
            to,
            total,
            algo,
            ..EscrowEntry::default()
        };
        self.total_locked += total;
        self.entries.insert(id, entry);
        id
    }

    pub fn lock(&mut self, from: String, to: String, total: u64) -> EscrowId {
        self.lock_with_algo(from, to, total, HashAlgo::Blake3)
    }

    pub fn status(&self, id: EscrowId) -> Option<&EscrowEntry> {
        self.entries.get(&id)
    }

    pub fn release(&mut self, id: EscrowId, amount: u64) -> Option<PaymentProof> {
        let (mut proof, algo, done, total) = {
            let entry = self.entries.get_mut(&id)?;
            if entry.released + amount > entry.total {
                return None;
            }
            entry.released += amount;
            entry.payments.push(amount);
            let leaves: Vec<[u8; 32]> = entry
                .payments
                .iter()
                .map(|a| hash_leaf(*a, entry.algo))
                .collect();
            entry.root = merkle_root(&leaves, entry.algo);
            let proof = build_proof(&leaves, leaves.len() - 1, entry.algo)?;
            let done = entry.released == entry.total;
            let total = entry.total;
            (proof, entry.algo, done, total)
        };
        if done {
            self.total_locked -= total;
            self.entries.remove(&id);
        }
        proof.algo = algo;
        Some(proof)
    }

    pub fn cancel(&mut self, id: EscrowId) -> bool {
        if let Some(e) = self.entries.remove(&id) {
            self.total_locked -= e.total;
            true
        } else {
            false
        }
    }

    pub fn total_locked(&self) -> u64 {
        self.total_locked
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn proof(&self, id: EscrowId, idx: usize) -> Option<PaymentProof> {
        let entry = self.entries.get(&id)?;
        let leaves: Vec<[u8; 32]> = entry
            .payments
            .iter()
            .map(|a| hash_leaf(*a, entry.algo))
            .collect();
        let mut proof = build_proof(&leaves, idx, entry.algo)?;
        proof.algo = entry.algo;
        Some(proof)
    }
}

fn hash_pair(a: [u8; 32], b: [u8; 32], algo: HashAlgo) -> [u8; 32] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&a);
    buf[32..].copy_from_slice(&b);
    match algo {
        HashAlgo::Blake3 => blake3::hash(&buf).into(),
        HashAlgo::Sha3 => {
            let mut hasher = Sha3_256::new();
            hasher.update(&buf);
            hasher.finalize().into()
        }
    }
}

fn hash_leaf(amount: u64, algo: HashAlgo) -> [u8; 32] {
    let bytes = amount.to_le_bytes();
    match algo {
        HashAlgo::Blake3 => blake3::hash(&bytes).into(),
        HashAlgo::Sha3 => {
            let mut hasher = Sha3_256::new();
            hasher.update(&bytes);
            hasher.finalize().into()
        }
    }
}

fn merkle_root(leaves: &[[u8; 32]], algo: HashAlgo) -> [u8; 32] {
    if leaves.is_empty() {
        return match algo {
            HashAlgo::Blake3 => blake3::hash(&[]).into(),
            HashAlgo::Sha3 => {
                let mut hasher = Sha3_256::new();
                hasher.update(&[]);
                hasher.finalize().into()
            }
        };
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for i in (0..level.len()).step_by(2) {
            let a = level[i];
            let b = if i + 1 < level.len() {
                level[i + 1]
            } else {
                level[i]
            };
            next.push(hash_pair(a, b, algo));
        }
        level = next;
    }
    level[0]
}

fn build_proof(leaves: &[[u8; 32]], idx: usize, algo: HashAlgo) -> Option<PaymentProof> {
    if leaves.is_empty() || idx >= leaves.len() {
        return None;
    }
    let mut path = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    let mut index = idx;
    while level.len() > 1 {
        let sibling = if index % 2 == 0 {
            if index + 1 < level.len() {
                level[index + 1]
            } else {
                level[index]
            }
        } else {
            level[index - 1]
        };
        path.push(sibling);
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for i in (0..level.len()).step_by(2) {
            let a = level[i];
            let b = if i + 1 < level.len() {
                level[i + 1]
            } else {
                level[i]
            };
            next.push(hash_pair(a, b, algo));
        }
        index /= 2;
        level = next;
    }
    Some(PaymentProof {
        leaf: leaves[idx],
        path,
        algo,
    })
}

pub fn verify_proof(
    leaf: [u8; 32],
    idx: usize,
    path: &[[u8; 32]],
    root: [u8; 32],
    algo: HashAlgo,
) -> bool {
    let mut hash = leaf;
    let mut index = idx;
    for sib in path {
        hash = if index % 2 == 0 {
            hash_pair(hash, *sib, algo)
        } else {
            hash_pair(*sib, hash, algo)
        };
        index /= 2;
    }
    hash.ct_eq(&root).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_roundtrip_blake3() {
        let mut es = Escrow::default();
        let id = es.lock("a".into(), "b".into(), 100);
        let proof = es.release(id, 40).unwrap();
        let entry = es.status(id).unwrap();
        assert!(verify_proof(
            proof.leaf,
            0,
            &proof.path,
            entry.root,
            proof.algo
        ));
    }

    #[test]
    fn proof_roundtrip_sha3() {
        let mut es = Escrow::default();
        let id = es.lock_with_algo("a".into(), "b".into(), 100, HashAlgo::Sha3);
        let proof = es.release(id, 40).unwrap();
        let entry = es.status(id).unwrap();
        assert!(verify_proof(
            proof.leaf,
            0,
            &proof.path,
            entry.root,
            proof.algo
        ));
    }
}
