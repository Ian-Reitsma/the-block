#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Unique identifier for an escrow entry.
pub type EscrowId = u64;

/// A Merkle proof for a particular partial payment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentProof {
    /// Hash of the payment leaf.
    pub leaf: [u8; 32],
    /// Sibling hashes from leaf to root.
    pub path: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowEntry {
    pub from: String,
    pub to: String,
    pub total: u64,
    pub released: u64,
    pub payments: Vec<u64>,
    pub root: [u8; 32],
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
    pub fn lock(&mut self, from: String, to: String, total: u64) -> EscrowId {
        let id = self.next_id;
        self.next_id += 1;
        let entry = EscrowEntry {
            from,
            to,
            total,
            ..EscrowEntry::default()
        };
        self.total_locked += total;
        self.entries.insert(id, entry);
        id
    }

    pub fn status(&self, id: EscrowId) -> Option<&EscrowEntry> {
        self.entries.get(&id)
    }

    pub fn release(&mut self, id: EscrowId, amount: u64) -> Option<PaymentProof> {
        let entry = self.entries.get_mut(&id)?;
        if entry.released + amount > entry.total {
            return None;
        }
        entry.released += amount;
        entry.payments.push(amount);
        let leaves: Vec<[u8; 32]> = entry
            .payments
            .iter()
            .map(|a| blake3::hash(&a.to_le_bytes()).into())
            .collect();
        entry.root = merkle_root(&leaves);
        let proof = build_proof(&leaves, leaves.len() - 1);
        if entry.released == entry.total {
            self.total_locked -= entry.total;
            self.entries.remove(&id);
        }
        proof
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
            .map(|a| blake3::hash(&a.to_le_bytes()).into())
            .collect();
        build_proof(&leaves, idx)
    }
}

fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return blake3::hash(&[]).into();
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for i in (0..level.len()).step_by(2) {
            let a = level[i];
            let b = if i + 1 < level.len() { level[i + 1] } else { level[i] };
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(&a);
            buf[32..].copy_from_slice(&b);
            next.push(blake3::hash(&buf).into());
        }
        level = next;
    }
    level[0]
}

fn build_proof(leaves: &[[u8; 32]], idx: usize) -> Option<PaymentProof> {
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
            let b = if i + 1 < level.len() { level[i + 1] } else { level[i] };
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(&a);
            buf[32..].copy_from_slice(&b);
            next.push(blake3::hash(&buf).into());
        }
        index /= 2;
        level = next;
    }
    Some(PaymentProof {
        leaf: leaves[idx],
        path,
    })
}

pub fn verify_proof(leaf: [u8; 32], idx: usize, path: &[[u8; 32]], root: [u8; 32]) -> bool {
    let mut hash = leaf;
    let mut index = idx;
    for sib in path {
        let mut buf = [0u8; 64];
        if index % 2 == 0 {
            buf[..32].copy_from_slice(&hash);
            buf[32..].copy_from_slice(sib);
        } else {
            buf[..32].copy_from_slice(sib);
            buf[32..].copy_from_slice(&hash);
        }
        hash = blake3::hash(&buf).into();
        index /= 2;
    }
    hash == root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_roundtrip() {
        let mut es = Escrow::default();
        let id = es.lock("a".into(), "b".into(), 100);
        let proof = es.release(id, 40).unwrap();
        let entry = es.status(id).unwrap();
        assert!(verify_proof(proof.leaf, 0, &proof.path, entry.root));
    }
}

