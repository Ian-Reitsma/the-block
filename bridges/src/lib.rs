#![forbid(unsafe_code)]

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub mod header;
pub mod light_client;
pub mod lock;
pub mod relayer;
pub mod unlock;

use header::PowHeader;
use light_client::Proof;
use relayer::RelayerSet;

pub use header::PowHeader as BridgeHeader;
pub use relayer::{Relayer, RelayerSet as Relayers};

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

#[cfg(feature = "telemetry")]
pub static BRIDGE_INVALID_PROOF_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "bridge_invalid_proof_total",
        "Bridge proofs rejected as invalid",
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

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub confirm_depth: u64,
    pub fee_per_byte: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            confirm_depth: 6,
            fee_per_byte: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Bridge {
    pub locked: HashMap<String, u64>,
    #[serde(default)]
    pub verified_headers: HashSet<[u8; 32]>,
    #[serde(skip)]
    pub cfg: BridgeConfig,
}

impl Default for Bridge {
    fn default() -> Self {
        Self {
            locked: HashMap::new(),
            verified_headers: HashSet::new(),
            cfg: BridgeConfig::default(),
        }
    }
}

impl Bridge {
    pub fn locked(&self, user: &str) -> u64 {
        self.locked.get(user).copied().unwrap_or(0)
    }

    pub fn deposit_with_relayer(
        &mut self,
        relayers: &mut RelayerSet,
        relayer: &str,
        user: &str,
        amount: u64,
        header: &PowHeader,
        proof: &Proof,
        rproof: &RelayerProof,
    ) -> bool {
        lock::lock(self, relayers, relayer, user, amount, header, proof, rproof)
    }

    pub fn unlock_with_relayer(
        &mut self,
        relayers: &mut RelayerSet,
        relayer: &str,
        user: &str,
        amount: u64,
        rproof: &RelayerProof,
    ) -> bool {
        unlock::unlock(self, relayers, relayer, user, amount, rproof)
    }
}
