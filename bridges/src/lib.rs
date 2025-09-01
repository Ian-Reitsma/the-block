#![forbid(unsafe_code)]

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayerProof {
    pub relayer: String,
    pub commitment: [u8; 32],
}

impl RelayerProof {
    pub fn new(relayer: &str, user: &str, amount: u64) -> Self {
        let mut h = Hasher::new();
        h.update(relayer.as_bytes());
        h.update(user.as_bytes());
        h.update(&amount.to_le_bytes());
        Self {
            relayer: relayer.to_string(),
            commitment: *h.finalize().as_bytes(),
        }
    }
    pub fn verify(&self, user: &str, amount: u64) -> bool {
        let expected = RelayerProof::new(&self.relayer, user, amount);
        self.commitment == expected.commitment
    }
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Bridge {
    locked: HashMap<String, u64>,
}

impl Bridge {
    pub fn lock(&mut self, user: &str, amount: u64, proof: &RelayerProof) -> bool {
        if !proof.verify(user, amount) {
            return false;
        }
        *self.locked.entry(user.to_string()).or_insert(0) += amount;
        true
    }
    pub fn unlock(&mut self, user: &str, amount: u64, proof: &RelayerProof) -> bool {
        if !proof.verify(user, amount) {
            return false;
        }
        let entry = self.locked.entry(user.to_string()).or_insert(0);
        if *entry < amount {
            return false;
        }
        *entry -= amount;
        true
    }
    pub fn locked(&self, user: &str) -> u64 {
        self.locked.get(user).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_unlock_with_proof() {
        let mut b = Bridge::default();
        let proof = RelayerProof::new("relayer", "alice", 50);
        assert!(b.lock("alice", 50, &proof));
        assert_eq!(b.locked("alice"), 50);
        assert!(b.unlock("alice", 50, &proof));
        assert_eq!(b.locked("alice"), 0);
    }
}
