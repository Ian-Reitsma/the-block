#![forbid(unsafe_code)]

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub mod light_client;
use light_client::{header_hash, Header, Proof};

#[cfg(feature = "telemetry")]
use once_cell::sync::Lazy;
#[cfg(feature = "telemetry")]
use prometheus::{IntCounter, Opts, Registry};

#[cfg(feature = "telemetry")]
static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

#[cfg(feature = "telemetry")]
pub static PROOF_VERIFY_SUCCESS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "bridge_proof_verify_success_total",
        "Bridge proofs successfully verified",
    ))
    .expect("counter");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

#[cfg(feature = "telemetry")]
pub static PROOF_VERIFY_FAILURE_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "bridge_proof_verify_failure_total",
        "Bridge proofs rejected",
    ))
    .expect("counter");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

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
    #[serde(default)]
    verified_headers: HashSet<[u8; 32]>,
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
    pub fn deposit_verified(
        &mut self,
        user: &str,
        amount: u64,
        header: &Header,
        proof: &Proof,
    ) -> bool {
        if !verify_header(header, proof) {
            #[cfg(feature = "telemetry")]
            PROOF_VERIFY_FAILURE_TOTAL.inc();
            return false;
        }
        let h = header_hash(header);
        if !self.verified_headers.insert(h) {
            #[cfg(feature = "telemetry")]
            PROOF_VERIFY_FAILURE_TOTAL.inc();
            return false;
        }
        *self.locked.entry(user.to_string()).or_insert(0) += amount;
        #[cfg(feature = "telemetry")]
        PROOF_VERIFY_SUCCESS_TOTAL.inc();
        true
    }
    pub fn locked(&self, user: &str) -> u64 {
        self.locked.get(user).copied().unwrap_or(0)
    }
}

pub fn verify_header(header: &Header, proof: &Proof) -> bool {
    light_client::verify(header, proof)
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
