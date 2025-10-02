#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod header;
pub mod light_client;
pub mod lock;
pub mod relayer;
pub mod token_bridge;
pub mod unlock;

use header::PowHeader;
use light_client::Proof;
use relayer::RelayerSet;

pub use header::PowHeader as BridgeHeader;
pub use relayer::{Relayer, RelayerSet as Relayers};
pub use token_bridge::TokenBridge;

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

#[cfg(feature = "telemetry")]
pub static BRIDGE_CHALLENGES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(
        Opts::new(
            "bridge_challenges_total",
            "Number of bridge withdrawals challenged",
        )
        .namespace("bridge"),
    )
    .expect("counter");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

#[cfg(feature = "telemetry")]
pub static BRIDGE_SLASHES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(
        Opts::new(
            "bridge_slashes_total",
            "Relayer slashes triggered by bridge security rules",
        )
        .namespace("bridge"),
    )
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayerBundle {
    pub proofs: Vec<RelayerProof>,
}

impl RelayerBundle {
    pub fn new(proofs: Vec<RelayerProof>) -> Self {
        Self { proofs }
    }

    pub fn verify(&self, user: &str, amount: u64) -> (usize, Vec<String>) {
        let mut valid = 0;
        let mut invalid = Vec::new();
        for proof in &self.proofs {
            if proof.verify(user, amount) {
                valid += 1;
            } else {
                invalid.push(proof.relayer.clone());
            }
        }
        (valid, invalid)
    }

    pub fn relayer_ids(&self) -> Vec<String> {
        self.proofs.iter().map(|p| p.relayer.clone()).collect()
    }

    pub fn aggregate_commitment(&self, user: &str, amount: u64) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(user.as_bytes());
        h.update(&amount.to_le_bytes());
        let mut relayers: Vec<_> = self.proofs.iter().map(|p| p.relayer.as_bytes()).collect();
        relayers.sort();
        for rel in relayers {
            h.update(rel);
        }
        *h.finalize().as_bytes()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWithdrawal {
    pub user: String,
    pub amount: u64,
    pub relayers: Vec<String>,
    pub initiated_at: u64,
    pub challenged: bool,
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub confirm_depth: u64,
    pub fee_per_byte: u64,
    pub headers_dir: String,
    pub challenge_period_secs: u64,
    pub relayer_quorum: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            confirm_depth: 6,
            fee_per_byte: 0,
            headers_dir: "state/bridge_headers".into(),
            challenge_period_secs: 30,
            relayer_quorum: 2,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Bridge {
    pub locked: HashMap<String, u64>,
    #[serde(default)]
    pub verified_headers: HashSet<[u8; 32]>,
    #[serde(default)]
    pub pending_withdrawals: HashMap<[u8; 32], PendingWithdrawal>,
    #[serde(skip)]
    pub cfg: BridgeConfig,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new(BridgeConfig::default())
    }
}

impl Bridge {
    fn load_headers(dir: &str) -> HashSet<[u8; 32]> {
        let mut set = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if let Ok(bytes) = std::fs::read(e.path()) {
                    if let Ok(hdr) = serde_json::from_slice::<PowHeader>(&bytes) {
                        let h = light_client::Header {
                            chain_id: hdr.chain_id.clone(),
                            height: hdr.height,
                            merkle_root: hdr.merkle_root,
                            signature: hdr.signature,
                        };
                        set.insert(light_client::header_hash(&h));
                    }
                }
            }
        }
        set
    }

    pub fn new(cfg: BridgeConfig) -> Self {
        let headers = Self::load_headers(&cfg.headers_dir);
        Self {
            locked: HashMap::new(),
            verified_headers: headers,
            pending_withdrawals: HashMap::new(),
            cfg,
        }
    }

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
        bundle: &RelayerBundle,
    ) -> bool {
        lock::lock(self, relayers, relayer, user, amount, header, proof, bundle)
    }

    pub fn unlock_with_relayer(
        &mut self,
        relayers: &mut RelayerSet,
        relayer: &str,
        user: &str,
        amount: u64,
        bundle: &RelayerBundle,
    ) -> bool {
        unlock::unlock(self, relayers, relayer, user, amount, bundle)
    }

    pub fn challenge_withdrawal(
        &mut self,
        relayers: &mut RelayerSet,
        commitment: [u8; 32],
    ) -> bool {
        if let Some(pending) = self.pending_withdrawals.get_mut(&commitment) {
            if pending.challenged {
                return false;
            }
            pending.challenged = true;
            *self.locked.entry(pending.user.clone()).or_insert(0) += pending.amount;
            for rel in &pending.relayers {
                relayers.slash(rel, 1);
            }
            #[cfg(feature = "telemetry")]
            {
                BRIDGE_CHALLENGES_TOTAL.inc();
            }
            true
        } else {
            false
        }
    }

    pub fn finalize_withdrawal(&mut self, commitment: [u8; 32]) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(pending) = self.pending_withdrawals.get(&commitment) {
            if pending.challenged {
                return false;
            }
            if now < pending.initiated_at + self.cfg.challenge_period_secs {
                return false;
            }
        } else {
            return false;
        }
        self.pending_withdrawals.remove(&commitment).is_some()
    }

    pub fn pending_withdrawals(&self) -> Vec<([u8; 32], PendingWithdrawal)> {
        self.pending_withdrawals
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }
}

/// Detect whether a given output script encodes an HTLC.
///
/// Scripts follow the format `htlc:<hexhash>:<timeout>` where `<hexhash>`
/// may be either 20-byte (RIPEMD) or 32-byte (SHA3) and `<timeout>` is a
/// decimal integer.
pub fn is_htlc_output(script: &[u8]) -> bool {
    let s = match std::str::from_utf8(script) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut parts = s.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("htlc"), Some(hash_hex), Some(timeout), None) => {
            let hash_bytes = match hex::decode(hash_hex) {
                Ok(b) => b,
                Err(_) => return false,
            };
            if hash_bytes.len() != 20 && hash_bytes.len() != 32 {
                return false;
            }
            timeout.parse::<u64>().is_ok()
        }
        _ => false,
    }
}
