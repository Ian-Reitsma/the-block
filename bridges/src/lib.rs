#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod codec;
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
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::{Counter, CounterVec};

#[cfg(feature = "telemetry")]
mod telemetry_support {
    use concurrency::Lazy;
    use runtime::telemetry::{Counter, CounterVec, Opts, Registry};

    pub(super) static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

    pub(super) fn counter(name: &'static str, help: &'static str) -> Counter {
        REGISTRY
            .get()
            .register_counter(name, help)
            .expect("register bridge telemetry counter")
    }

    pub(super) fn counter_vec(
        name: &'static str,
        help: &'static str,
        labels: &'static [&'static str],
    ) -> CounterVec {
        let opts = Opts::new(name, help);
        let vec = CounterVec::new(opts, labels).expect("create bridge telemetry counter vec");
        REGISTRY
            .get()
            .register(Box::new(vec.clone()))
            .expect("register bridge telemetry counter vec");
        vec
    }
}

#[cfg(feature = "telemetry")]
use telemetry_support::{counter, counter_vec};

#[cfg(feature = "telemetry")]
fn proof_verify_success_counter() -> Counter {
    counter(
        "bridge_proof_verify_success_total",
        "Bridge proofs successfully verified",
    )
}

#[cfg(feature = "telemetry")]
fn proof_verify_failure_counter() -> Counter {
    counter(
        "bridge_proof_verify_failure_total",
        "Bridge proofs rejected",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_invalid_proof_counter() -> Counter {
    counter(
        "bridge_invalid_proof_total",
        "Bridge proofs rejected as invalid",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_challenges_counter() -> Counter {
    counter(
        "bridge_challenges_total",
        "Number of bridge withdrawals challenged",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_slashes_counter() -> Counter {
    counter(
        "bridge_slashes_total",
        "Relayer slashes triggered by bridge security rules",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_reward_claims_counter() -> Counter {
    counter(
        "bridge_reward_claims_total",
        "Bridge reward claim operations processed",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_reward_approvals_consumed_counter() -> Counter {
    counter(
        "bridge_reward_approvals_consumed_total",
        "Total bridge reward allowance consumed by approved claims",
    )
}

#[cfg(feature = "telemetry")]
fn bridge_settlement_results_counter() -> CounterVec {
    counter_vec(
        "bridge_settlement_results_total",
        "Bridge settlement submissions grouped by result and reason",
        &["result", "reason"],
    )
}

#[cfg(feature = "telemetry")]
fn bridge_dispute_outcomes_counter() -> CounterVec {
    counter_vec(
        "bridge_dispute_outcomes_total",
        "Bridge dispute outcomes grouped by duty kind and outcome",
        &["kind", "outcome"],
    )
}

#[cfg(feature = "telemetry")]
fn bridge_liquidity_locked_counter() -> CounterVec {
    counter_vec(
        "bridge_liquidity_locked_total",
        "Bridge liquidity locked grouped by asset",
        &["asset"],
    )
}

#[cfg(feature = "telemetry")]
fn bridge_liquidity_unlocked_counter() -> CounterVec {
    counter_vec(
        "bridge_liquidity_unlocked_total",
        "Bridge liquidity unlocked grouped by asset",
        &["asset"],
    )
}

#[cfg(feature = "telemetry")]
fn bridge_liquidity_minted_counter() -> CounterVec {
    counter_vec(
        "bridge_liquidity_minted_total",
        "Bridge liquidity minted grouped by asset",
        &["asset"],
    )
}

#[cfg(feature = "telemetry")]
fn bridge_liquidity_burned_counter() -> CounterVec {
    counter_vec(
        "bridge_liquidity_burned_total",
        "Bridge liquidity burned grouped by asset",
        &["asset"],
    )
}

#[cfg(feature = "telemetry")]
pub static PROOF_VERIFY_SUCCESS_TOTAL: Lazy<Counter> = Lazy::new(proof_verify_success_counter);

#[cfg(feature = "telemetry")]
pub static PROOF_VERIFY_FAILURE_TOTAL: Lazy<Counter> = Lazy::new(proof_verify_failure_counter);

#[cfg(feature = "telemetry")]
static PROOF_COUNTER_TEST_GUARD: Lazy<std::sync::Mutex<()>> =
    Lazy::new(|| std::sync::Mutex::new(()));

#[cfg(feature = "telemetry")]
pub fn proof_counter_test_guard() -> std::sync::MutexGuard<'static, ()> {
    PROOF_COUNTER_TEST_GUARD
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

#[cfg(feature = "telemetry")]
pub static BRIDGE_INVALID_PROOF_TOTAL: Lazy<Counter> = Lazy::new(bridge_invalid_proof_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_CHALLENGES_TOTAL: Lazy<Counter> = Lazy::new(bridge_challenges_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_SLASHES_TOTAL: Lazy<Counter> = Lazy::new(bridge_slashes_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_REWARD_CLAIMS_TOTAL: Lazy<Counter> = Lazy::new(bridge_reward_claims_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL: Lazy<Counter> =
    Lazy::new(bridge_reward_approvals_consumed_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_SETTLEMENT_RESULTS_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_settlement_results_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_DISPUTE_OUTCOMES_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_dispute_outcomes_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_LIQUIDITY_LOCKED_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_liquidity_locked_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_LIQUIDITY_UNLOCKED_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_liquidity_unlocked_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_LIQUIDITY_MINTED_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_liquidity_minted_counter);

#[cfg(feature = "telemetry")]
pub static BRIDGE_LIQUIDITY_BURNED_TOTAL: Lazy<CounterVec> =
    Lazy::new(bridge_liquidity_burned_counter);

#[cfg(feature = "telemetry")]
pub(crate) fn telemetry_counter(name: &'static str, help: &'static str) -> Counter {
    counter(name, help)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
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

#[derive(Clone)]
pub struct Bridge {
    pub locked: HashMap<String, u64>,
    pub verified_headers: HashSet<[u8; 32]>,
    pub pending_withdrawals: HashMap<[u8; 32], PendingWithdrawal>,
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
                    if let Ok(value) = json::value_from_slice(&bytes) {
                        if let Ok(hdr) = PowHeader::from_value(&value) {
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
            let hash_bytes = match crypto_suite::hex::decode(hash_hex) {
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
