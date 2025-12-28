//! Receipt validation with cryptographic signatures and anti-forgery protections
//!
//! This module enforces:
//! - Signature verification via provider registry
//! - Nonce-based replay attack prevention
//! - Receipt deduplication across blocks
//! - Field validation and DoS limits

use crate::receipt_crypto::{
    verify_receipt_signature, CryptoError, NonceTracker, ProviderRegistry,
};
use crate::receipts::Receipt;
use crypto_suite::hashing::blake3;
use foundation_serialization::{Deserialize, Serialize};

/// Maximum number of receipts allowed per block (DoS protection)
pub const MAX_RECEIPTS_PER_BLOCK: usize = 10_000;

/// Maximum total serialized receipt bytes per block (10MB limit)
pub const MAX_RECEIPT_BYTES_PER_BLOCK: usize = 10_000_000;

/// Maximum length for string fields (contract_id, provider, etc.)
pub const MAX_STRING_FIELD_LENGTH: usize = 256;

/// Minimum BLOCK payment amount to emit a receipt (spam protection)
pub const MIN_PAYMENT_FOR_RECEIPT: u64 = 1;

/// Receipt-level validation error
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum ValidationError {
    TooManyReceipts {
        count: usize,
        max: usize,
    },
    ReceiptsTooLarge {
        bytes: usize,
        max: usize,
    },
    BlockHeightMismatch {
        receipt_height: u64,
        block_height: u64,
    },
    EmptyStringField {
        field: String,
    },
    StringFieldTooLong {
        field: String,
        length: usize,
        max: usize,
    },
    ZeroValue {
        field: String,
    },
    MissingSignature,
    InvalidSignature {
        reason: String,
    },
    UnknownProvider {
        provider_id: String,
    },
    ReplayedNonce {
        provider_id: String,
        nonce: u64,
    },
    DuplicateReceipt,
    EmptySignature,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TooManyReceipts { count, max } => {
                write!(f, "Too many receipts: {} (max: {})", count, max)
            }
            ValidationError::ReceiptsTooLarge { bytes, max } => {
                write!(f, "Receipts too large: {} bytes (max: {})", bytes, max)
            }
            ValidationError::BlockHeightMismatch {
                receipt_height,
                block_height,
            } => {
                write!(
                    f,
                    "Receipt height {} != block height {}",
                    receipt_height, block_height
                )
            }
            ValidationError::EmptyStringField { field } => {
                write!(f, "Empty string field: {}", field)
            }
            ValidationError::StringFieldTooLong { field, length, max } => {
                write!(
                    f,
                    "Field {} too long: {} chars (max: {})",
                    field, length, max
                )
            }
            ValidationError::ZeroValue { field } => write!(f, "Zero value: {}", field),
            ValidationError::MissingSignature => write!(f, "Missing signature"),
            ValidationError::InvalidSignature { reason } => {
                write!(f, "Invalid signature: {}", reason)
            }
            ValidationError::UnknownProvider { provider_id } => {
                write!(f, "Unknown provider: {}", provider_id)
            }
            ValidationError::ReplayedNonce { provider_id, nonce } => {
                write!(f, "Replayed nonce {} for provider {}", nonce, provider_id)
            }
            ValidationError::DuplicateReceipt => write!(f, "Duplicate receipt"),
            ValidationError::EmptySignature => write!(f, "Empty signature bytes"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Receipt identity for deduplication
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptId(pub [u8; 32]);

impl ReceiptId {
    pub fn from_receipt(receipt: &Receipt) -> Self {
        let mut hasher = blake3::Hasher::new();

        match receipt {
            Receipt::Storage(r) => {
                hasher.update(b"storage");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.contract_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::Compute(r) => {
                hasher.update(b"compute");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.job_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::Energy(r) => {
                hasher.update(b"energy");
                hasher.update(r.provider.as_bytes());
                hasher.update(r.contract_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
            Receipt::Ad(r) => {
                hasher.update(b"ad");
                hasher.update(r.publisher.as_bytes());
                hasher.update(r.campaign_id.as_bytes());
                hasher.update(&r.block_height.to_le_bytes());
                hasher.update(&r.signature_nonce.to_le_bytes());
            }
        }

        ReceiptId(hasher.finalize().into())
    }
}

/// Receipt registry for deduplication across blocks
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ReceiptRegistry {
    ids: std::collections::HashSet<ReceiptId>,
}

impl ReceiptRegistry {
    pub fn new() -> Self {
        Self {
            ids: std::collections::HashSet::new(),
        }
    }

    pub fn register(&mut self, id: ReceiptId) -> Result<(), ValidationError> {
        if !self.ids.insert(id) {
            return Err(ValidationError::DuplicateReceipt);
        }
        Ok(())
    }

    pub fn prune_with<F>(&mut self, mut should_remove: F)
    where
        F: FnMut(&ReceiptId) -> bool,
    {
        self.ids.retain(|id| !should_remove(id));
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

fn receipt_provider_and_nonce(receipt: &Receipt) -> (&str, u64) {
    match receipt {
        Receipt::Storage(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::Compute(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::Energy(r) => (r.provider.as_str(), r.signature_nonce),
        Receipt::Ad(r) => (r.publisher.as_str(), r.signature_nonce),
    }
}

/// Validate receipt with full cryptographic checks
pub fn validate_receipt(
    receipt: &Receipt,
    block_height: u64,
    provider_registry: &ProviderRegistry,
    nonce_tracker: &mut NonceTracker,
) -> Result<(), ValidationError> {
    let (provider_id, nonce) = receipt_provider_and_nonce(receipt);
    if nonce_tracker.has_seen_nonce(provider_id, nonce) {
        return Err(ValidationError::ReplayedNonce {
            provider_id: provider_id.to_string(),
            nonce,
        });
    }

    // Check block height
    if receipt.block_height() != block_height {
        return Err(ValidationError::BlockHeightMismatch {
            receipt_height: receipt.block_height(),
            block_height,
        });
    }

    // Field validation
    match receipt {
        Receipt::Storage(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.bytes == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "bytes".to_string(),
                });
            }
            if r.price == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "price".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::Compute(r) => {
            validate_string_field("job_id", &r.job_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.compute_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "compute_units".to_string(),
                });
            }
            if r.payment == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "payment".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::Energy(r) => {
            validate_string_field("contract_id", &r.contract_id)?;
            validate_string_field("provider", &r.provider)?;
            if r.energy_units == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "energy_units".to_string(),
                });
            }
            if r.price == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "price".to_string(),
                });
            }
            if r.provider_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
        Receipt::Ad(r) => {
            validate_string_field("campaign_id", &r.campaign_id)?;
            validate_string_field("publisher", &r.publisher)?;
            if r.impressions == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "impressions".to_string(),
                });
            }
            if r.spend == 0 {
                return Err(ValidationError::ZeroValue {
                    field: "spend".to_string(),
                });
            }
            if r.publisher_signature.is_empty() {
                return Err(ValidationError::EmptySignature);
            }
        }
    }

    // Cryptographic signature verification
    verify_receipt_signature(receipt, provider_registry, nonce_tracker, block_height).map_err(|e| {
        match e {
            CryptoError::InvalidSignature { reason } => {
                ValidationError::InvalidSignature { reason }
            }
            CryptoError::UnknownProvider { provider_id } => {
                ValidationError::UnknownProvider { provider_id }
            }
            CryptoError::ReplayedNonce { provider_id, nonce } => {
                ValidationError::ReplayedNonce { provider_id, nonce }
            }
            CryptoError::MalformedSignature { reason } => {
                ValidationError::InvalidSignature { reason }
            }
        }
    })
}

fn validate_string_field(field_name: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyStringField {
            field: field_name.to_string(),
        });
    }
    if value.len() > MAX_STRING_FIELD_LENGTH {
        return Err(ValidationError::StringFieldTooLong {
            field: field_name.to_string(),
            length: value.len(),
            max: MAX_STRING_FIELD_LENGTH,
        });
    }
    Ok(())
}

pub fn validate_receipt_count(count: usize) -> Result<(), ValidationError> {
    if count > MAX_RECEIPTS_PER_BLOCK {
        return Err(ValidationError::TooManyReceipts {
            count,
            max: MAX_RECEIPTS_PER_BLOCK,
        });
    }
    Ok(())
}

pub fn validate_receipt_size(bytes: usize) -> Result<(), ValidationError> {
    if bytes > MAX_RECEIPT_BYTES_PER_BLOCK {
        return Err(ValidationError::ReceiptsTooLarge {
            bytes,
            max: MAX_RECEIPT_BYTES_PER_BLOCK,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipts::{ComputeReceipt, StorageReceipt};
    use crypto_suite::signatures::ed25519::SigningKey;
    use rand::{rngs::StdRng, SeedableRng};

    fn create_signed_storage_receipt(
        sk: &SigningKey,
        block_height: u64,
        nonce: u64,
    ) -> StorageReceipt {
        let mut receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height,
            provider_escrow: 10000,
            provider_signature: vec![],
            signature_nonce: nonce,
        };

        // Build preimage
        use crypto_suite::hashing::blake3;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"storage");
        hasher.update(&receipt.block_height.to_le_bytes());
        hasher.update(receipt.contract_id.as_bytes());
        hasher.update(receipt.provider.as_bytes());
        hasher.update(&receipt.bytes.to_le_bytes());
        hasher.update(&receipt.price.to_le_bytes());
        hasher.update(&receipt.provider_escrow.to_le_bytes());
        hasher.update(&receipt.signature_nonce.to_le_bytes());
        let preimage = hasher.finalize();

        let signature = sk.sign(preimage.as_bytes());
        receipt.provider_signature = signature.to_bytes().to_vec();
        receipt
    }

    #[test]
    fn valid_receipt_passes() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        assert!(validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker
        )
        .is_ok());
    }

    #[test]
    fn forged_signature_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let mut receipt = create_signed_storage_receipt(&sk, 100, 1);
        // Corrupt signature
        receipt.provider_signature[0] ^= 0xFF;

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(
            result,
            Err(ValidationError::InvalidSignature { .. })
        ));
    }

    #[test]
    fn unsigned_receipt_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = StorageReceipt {
            contract_id: "contract_001".into(),
            provider: "provider_001".into(),
            bytes: 1_000_000,
            price: 500,
            block_height: 100,
            provider_escrow: 10000,
            provider_signature: vec![], // Empty
            signature_nonce: 1,
        };

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(result, Err(ValidationError::EmptySignature)));
    }

    #[test]
    fn replay_attack_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let mut registry = ProviderRegistry::new();
        registry
            .register_provider("provider_001".into(), vk, 0)
            .unwrap();
        let mut nonce_tracker = NonceTracker::new(100);

        // First validation succeeds
        assert!(validate_receipt(
            &Receipt::Storage(receipt.clone()),
            100,
            &registry,
            &mut nonce_tracker
        )
        .is_ok());

        // Second validation with same nonce fails
        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(result, Err(ValidationError::ReplayedNonce { .. })));
    }

    #[test]
    fn duplicate_receipt_detected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);

        let receipt = create_signed_storage_receipt(&sk, 100, 1);
        let id = ReceiptId::from_receipt(&Receipt::Storage(receipt));

        let mut registry = ReceiptRegistry::new();
        assert!(registry.register(id).is_ok());
        assert!(matches!(
            registry.register(id),
            Err(ValidationError::DuplicateReceipt)
        ));
    }

    #[test]
    fn unknown_provider_rejected() {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);

        let receipt = create_signed_storage_receipt(&sk, 100, 1);

        let registry = ProviderRegistry::new(); // Empty registry
        let mut nonce_tracker = NonceTracker::new(100);

        let result = validate_receipt(
            &Receipt::Storage(receipt),
            100,
            &registry,
            &mut nonce_tracker,
        );
        assert!(matches!(
            result,
            Err(ValidationError::UnknownProvider { .. })
        ));
    }
}
