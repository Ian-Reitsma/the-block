// Receipt Cryptographic Verification Layer
//
// This module handles Ed25519 signature verification for market receipts,
// preventing forged settlements and building confidence in economic metrics.

use crate::blocktorch_accelerator::global_blocktorch_accelerator;
use crate::receipts::{AdReceipt, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt};
use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::convert::TryInto;
use std::hash::{Hash, Hasher};

/// Provider registration tracking public keys for receipt verification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderRegistry {
    /// Map of provider_id -> (public_key, registered_at_block)
    pub providers: HashMap<String, ProviderRecord>,
}

/// Provider metadata attached to a verifying key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderRecord {
    pub verifying_key: VerifyingKey,
    pub registered_at_block: u64,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub asn: Option<u32>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider's public key
    pub fn register_provider(
        &mut self,
        provider_id: String,
        verifying_key: VerifyingKey,
        block_height: u64,
    ) -> Result<(), String> {
        self.register_provider_with_metadata(provider_id, verifying_key, block_height, None, None)
    }

    /// Register provider metadata (region/ASN optional).
    pub fn register_provider_with_metadata(
        &mut self,
        provider_id: String,
        verifying_key: VerifyingKey,
        block_height: u64,
        region: Option<String>,
        asn: Option<u32>,
    ) -> Result<(), String> {
        if provider_id.is_empty() {
            return Err("provider_id cannot be empty".into());
        }
        if provider_id.len() > 256 {
            return Err("provider_id too long".into());
        }

        self.providers.insert(
            provider_id,
            ProviderRecord {
                verifying_key,
                registered_at_block: block_height,
                region,
                asn,
            },
        );
        Ok(())
    }

    /// Retrieve provider's public key
    pub fn get_provider(&self, provider_id: &str) -> Option<VerifyingKey> {
        self.providers
            .get(provider_id)
            .map(|record| record.verifying_key.clone())
    }

    pub fn get_provider_record(&self, provider_id: &str) -> Option<&ProviderRecord> {
        self.providers.get(provider_id)
    }

    pub fn provider_registered(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }
}

const MAX_NONCES_TRACKED: usize = 1 << 12;
const NONCE_KEY_DOMAIN: &[u8] = b"receipt_nonce";

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct NonceKey([u8; 32]);

impl NonceKey {
    fn new(provider_id: &str, nonce: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(NONCE_KEY_DOMAIN);
        hasher.update(provider_id.as_bytes());
        hasher.update(&nonce.to_le_bytes());
        Self(hasher.finalize().into())
    }
}

impl PartialEq for NonceKey {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for NonceKey {}

impl Hash for NonceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0);
    }
}

/// Nonce tracking to prevent replay attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct NonceTracker {
    seen_nonces: HashMap<NonceKey, u64>,
    ordered_keys: VecDeque<(u64, NonceKey)>,
    /// Finality window in blocks (prune nonces older than this)
    pub finality_window: u64,
}

impl NonceTracker {
    pub fn new(finality_window: u64) -> Self {
        Self {
            seen_nonces: HashMap::new(),
            ordered_keys: VecDeque::new(),
            finality_window,
        }
    }

    /// Check if nonce has been used; record if not seen
    pub fn has_seen_nonce(&self, provider_id: &str, nonce: u64) -> bool {
        self.seen_nonces
            .contains_key(&NonceKey::new(provider_id, nonce))
    }

    pub fn check_and_record_nonce(
        &mut self,
        provider_id: &str,
        nonce: u64,
        current_block: u64,
    ) -> Result<(), String> {
        let key = NonceKey::new(provider_id, nonce);

        if self.seen_nonces.contains_key(&key) {
            return Err(format!(
                "nonce {} already used for provider {}",
                nonce, provider_id
            ));
        }

        self.seen_nonces.insert(key, current_block);
        self.ordered_keys.push_back((current_block, key));
        self.enforce_capacity();
        Ok(())
    }

    /// Prune nonces older than finality window
    pub fn prune_old_nonces(&mut self, current_block: u64) {
        let cutoff = current_block.saturating_sub(self.finality_window);
        while let Some((block, key)) = self.ordered_keys.front() {
            if *block >= cutoff {
                break;
            }
            let (block, key) = self.ordered_keys.pop_front().unwrap();
            if self
                .seen_nonces
                .get(&key)
                .copied()
                .map_or(false, |stored| stored == block)
            {
                self.seen_nonces.remove(&key);
            }
        }
    }

    fn enforce_capacity(&mut self) {
        while self.ordered_keys.len() > MAX_NONCES_TRACKED {
            if let Some((block, key)) = self.ordered_keys.pop_front() {
                if self
                    .seen_nonces
                    .get(&key)
                    .copied()
                    .map_or(false, |stored| stored == block)
                {
                    self.seen_nonces.remove(&key);
                }
            }
        }
    }
}

/// Receipt cryptographic error types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum CryptoError {
    /// Signature verification failed
    InvalidSignature { reason: String },
    /// Provider not registered
    UnknownProvider { provider_id: String },
    /// Nonce has been used before (replay attack)
    ReplayedNonce { provider_id: String, nonce: u64 },
    /// Signature bytes malformed
    MalformedSignature { reason: String },
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature { reason } => write!(f, "Invalid signature: {}", reason),
            Self::UnknownProvider { provider_id } => {
                write!(f, "Unknown provider: {}", provider_id)
            }
            Self::ReplayedNonce { provider_id, nonce } => {
                write!(f, "Replayed nonce {} for provider {}", nonce, provider_id)
            }
            Self::MalformedSignature { reason } => write!(f, "Malformed signature: {}", reason),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Build deterministic preimage for storage receipt signature
fn build_storage_preimage(receipt: &StorageReceipt) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();

    hasher.update(b"storage");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.contract_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.bytes.to_le_bytes());
    hasher.update(&receipt.price.to_le_bytes());
    hasher.update(&receipt.provider_escrow.to_le_bytes());
    if let Some(chunk_hash) = &receipt.chunk_hash {
        hasher.update(chunk_hash);
    } else {
        hasher.update(b"chunk_hash:none");
    }
    if let Some(region) = &receipt.region {
        hasher.update(region.as_bytes());
    } else {
        hasher.update(b"region:none");
    }
    hasher.update(&receipt.signature_nonce.to_le_bytes());

    hasher.finalize().as_bytes().to_vec()
}

/// Build deterministic preimage for compute receipt signature
fn build_compute_preimage(receipt: &ComputeReceipt) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();

    hasher.update(b"compute");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.job_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.compute_units.to_le_bytes());
    hasher.update(&receipt.payment.to_le_bytes());
    hasher.update(&[u8::from(receipt.verified)]);
    hasher.update(&receipt.signature_nonce.to_le_bytes());
    if let Some(meta) = &receipt.blocktorch {
        hasher.update(&meta.kernel_variant_digest);
        if let Some(commit) = &meta.benchmark_commit {
            hasher.update(commit.as_bytes());
        }
        if let Some(epoch) = &meta.tensor_profile_epoch {
            hasher.update(epoch.as_bytes());
        }
        hasher.update(&meta.proof_latency_ms.to_le_bytes());
    } else {
        hasher.update(b"blocktorch:none");
    }

    hasher.finalize().as_bytes().to_vec()
}

/// Build deterministic preimage for energy receipt signature
fn build_energy_preimage(receipt: &EnergyReceipt) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();

    hasher.update(b"energy");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.contract_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.energy_units.to_le_bytes());
    hasher.update(&receipt.price.to_le_bytes());
    hasher.update(&receipt.proof_hash);
    hasher.update(&receipt.signature_nonce.to_le_bytes());

    hasher.finalize().as_bytes().to_vec()
}

/// Build deterministic preimage for ad receipt signature
fn build_ad_preimage(receipt: &AdReceipt) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();

    hasher.update(b"ad");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.campaign_id.as_bytes());
    hasher.update(receipt.publisher.as_bytes());
    hasher.update(&receipt.impressions.to_le_bytes());
    hasher.update(&receipt.spend.to_le_bytes());
    hasher.update(&receipt.conversions.to_le_bytes());
    hasher.update(&receipt.signature_nonce.to_le_bytes());

    hasher.finalize().as_bytes().to_vec()
}

/// Verify receipt signature against provider's registered public key
pub fn verify_receipt_signature(
    receipt: &Receipt,
    provider_registry: &ProviderRegistry,
    nonce_tracker: &mut NonceTracker,
    current_block: u64,
) -> Result<(), CryptoError> {
    if matches!(
        receipt,
        Receipt::EnergySlash(_)
            | Receipt::ComputeSlash(_)
            | Receipt::StorageSlash(_)
            | Receipt::Relay(_)
    ) {
        return Ok(());
    }
    // Get preimage and provider info based on receipt type
    let (preimage, provider_id, signature_bytes, nonce) = match receipt {
        Receipt::Storage(r) => (
            build_storage_preimage(r),
            r.provider.as_str(),
            &r.provider_signature,
            r.signature_nonce,
        ),
        Receipt::Compute(r) => (
            build_compute_preimage(r),
            r.provider.as_str(),
            &r.provider_signature,
            r.signature_nonce,
        ),
        Receipt::Energy(r) => (
            build_energy_preimage(r),
            r.provider.as_str(),
            &r.provider_signature,
            r.signature_nonce,
        ),
        Receipt::EnergySlash(_) => unreachable!("energy slash receipts are unsigned"),
        Receipt::ComputeSlash(_) => unreachable!("compute slash receipts are unsigned"),
        Receipt::StorageSlash(_) => unreachable!("storage slash receipts are unsigned"),
        Receipt::Ad(r) => (
            build_ad_preimage(r),
            r.publisher.as_str(),
            &r.publisher_signature,
            r.signature_nonce,
        ),
        Receipt::Relay(_) => return Ok(()),
    };

    // Get provider's public key
    let verifying_key =
        provider_registry
            .get_provider(provider_id)
            .ok_or(CryptoError::UnknownProvider {
                provider_id: provider_id.to_owned(),
            })?;

    // Check nonce hasn't been replayed
    nonce_tracker
        .check_and_record_nonce(provider_id, nonce, current_block)
        .map_err(|_| CryptoError::ReplayedNonce {
            provider_id: provider_id.to_owned(),
            nonce,
        })?;

    // Parse signature bytes
    let signature_array: [u8; 64] =
        signature_bytes
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::MalformedSignature {
                reason: "signature length mismatch or invalid encoding".into(),
            })?;
    let signature = Signature::from_bytes(&signature_array);

    // Verify signature via the configurable accelerator bridge.
    let accelerator = global_blocktorch_accelerator();
    accelerator
        .verify_signature(&preimage, &verifying_key, &signature)
        .map_err(|e| CryptoError::InvalidSignature {
            reason: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
    use rand::rngs::StdRng;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEED: AtomicU64 = AtomicU64::new(0);

    fn create_test_keypair() -> (SigningKey, VerifyingKey) {
        let seed = TEST_SEED.fetch_add(1, Ordering::SeqCst) + 1;
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    #[test]
    fn storage_receipt_signature_verifies() {
        let (sk, vk) = create_test_keypair();

        let mut receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            region: None,
            chunk_hash: None,
            provider_signature: vec![],
            signature_nonce: 1,
        };

        // Sign receipt
        let preimage = build_storage_preimage(&receipt);
        let signature = sk.sign(&preimage);
        receipt.provider_signature = signature.to_bytes().to_vec();

        // Verify
        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = verify_receipt_signature(
            &Receipt::Storage(receipt),
            &registry,
            &mut nonce_tracker,
            100,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn invalid_signature_rejected() {
        let (_, vk) = create_test_keypair();
        let (sk_attacker, _) = create_test_keypair();

        let mut receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            region: None,
            chunk_hash: None,
            provider_signature: vec![],
            signature_nonce: 1,
        };

        // Sign with wrong key
        let preimage = build_storage_preimage(&receipt);
        let signature = sk_attacker.sign(&preimage);
        receipt.provider_signature = signature.to_bytes().to_vec();

        // Try to verify with different key
        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = verify_receipt_signature(
            &Receipt::Storage(receipt),
            &registry,
            &mut nonce_tracker,
            100,
        );
        assert!(result.is_err());
    }

    #[test]
    fn replay_attack_rejected() {
        let (sk, vk) = create_test_keypair();

        let mut receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            region: None,
            chunk_hash: None,
            provider_signature: vec![],
            signature_nonce: 1,
        };

        let preimage = build_storage_preimage(&receipt);
        let signature = sk.sign(&preimage);
        receipt.provider_signature = signature.to_bytes().to_vec();

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        // First verification succeeds
        let result1 = verify_receipt_signature(
            &Receipt::Storage(receipt.clone()),
            &registry,
            &mut nonce_tracker,
            100,
        );
        assert!(result1.is_ok());

        // Second verification with same nonce fails (replay)
        let result2 = verify_receipt_signature(
            &Receipt::Storage(receipt),
            &registry,
            &mut nonce_tracker,
            100,
        );
        assert!(matches!(result2, Err(CryptoError::ReplayedNonce { .. })));
    }

    #[test]
    fn nonce_pruning_works() {
        let mut tracker = NonceTracker::new(100);

        // Record nonce at block 50
        tracker
            .check_and_record_nonce("provider_001", 1, 50)
            .unwrap();

        // At block 200, nonce should be pruned (200 - 100 = 100 > 50)
        tracker.prune_old_nonces(200);

        // Should allow reuse of pruned nonce
        assert!(tracker
            .check_and_record_nonce("provider_001", 1, 200)
            .is_ok());
    }

    #[test]
    fn nonce_capacity_enforced() {
        let mut tracker = NonceTracker::new(1_000_000);
        let extra = 10;
        for i in 0..(MAX_NONCES_TRACKED + extra) as u64 {
            let provider_id = format!("provider_{i}");
            tracker.check_and_record_nonce(&provider_id, i, i).unwrap();
        }
        assert!(tracker.seen_nonces.len() <= MAX_NONCES_TRACKED);
        assert!(tracker.ordered_keys.len() <= MAX_NONCES_TRACKED);
    }
}
