//! Receipt Security Tests - Anti-Forgery and Attack Resistance
//!
//! These tests encode security invariants that MUST hold before mainnet:
//! - Unsigned receipts are rejected
//! - Invalid signatures are rejected
//! - Duplicate receipts across blocks are rejected
//! - Replay attacks (reused nonces) are rejected
//! - Unknown providers cannot submit receipts

use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::StdRng;
use std::sync::atomic::{AtomicU64, Ordering};
use the_block::receipt_crypto::{NonceTracker, ProviderRegistry};
use the_block::receipts::{BlockTorchReceiptMetadata, ComputeReceipt, Receipt, StorageReceipt};
use the_block::receipts_validation::{
    validate_receipt, ReceiptId, ReceiptRegistry, ValidationError,
};

static TEST_SEED: AtomicU64 = AtomicU64::new(0);

fn create_test_keypair() -> (SigningKey, crypto_suite::signatures::ed25519::VerifyingKey) {
    let seed = TEST_SEED.fetch_add(1, Ordering::SeqCst) + 1;
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    (sk, vk)
}

fn sign_storage_receipt(receipt: &mut StorageReceipt, sk: &SigningKey) {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"storage");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.contract_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.bytes.to_le_bytes());
    hasher.update(&receipt.price.to_le_bytes());
    hasher.update(&receipt.provider_escrow.to_le_bytes());
    hasher.update(&receipt.signature_nonce.to_le_bytes());

    let msg = hasher.finalize();
    let sig = sk.sign(msg.as_bytes());
    receipt.provider_signature = sig.to_bytes().to_vec();
}

fn sign_compute_receipt(receipt: &mut ComputeReceipt, sk: &SigningKey) {
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

    let msg = hasher.finalize();
    let sig = sk.sign(msg.as_bytes());
    receipt.provider_signature = sig.to_bytes().to_vec();
}

fn sample_blocktorch_metadata(tag: &str) -> BlockTorchReceiptMetadata {
    let mut hasher = blake3::Hasher::new();
    hasher.update(tag.as_bytes());
    BlockTorchReceiptMetadata {
        kernel_variant_digest: *hasher.finalize().as_bytes(),
        descriptor_digest: {
            let mut descriptor_hasher = blake3::Hasher::new();
            descriptor_hasher.update(tag.as_bytes());
            descriptor_hasher.update(b"-descriptor");
            *descriptor_hasher.finalize().as_bytes()
        },
        output_digest: {
            let mut output_hasher = blake3::Hasher::new();
            output_hasher.update(tag.as_bytes());
            output_hasher.update(b"-output");
            *output_hasher.finalize().as_bytes()
        },
        benchmark_commit: Some(format!("{tag}-benchmark")),
        tensor_profile_epoch: Some(format!("{tag}-epoch")),
        proof_latency_ms: 7,
    }
}

#[test]
fn reject_unsigned_storage_receipt() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![], // UNSIGNED
        signature_nonce: 1,
    });

    let registry = ProviderRegistry::new();
    let mut nonce_tracker = NonceTracker::new(100);

    let result = validate_receipt(&receipt, 100, &registry, &mut nonce_tracker);
    assert!(matches!(result, Err(ValidationError::EmptySignature)));
}

#[test]
fn reject_invalid_signature() {
    let (_sk_legitimate, vk_legitimate) = create_test_keypair();
    let (sk_attacker, _) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    // Sign with attacker's key
    sign_storage_receipt(&mut receipt, &sk_attacker);

    // But registry has legitimate provider's key
    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk_legitimate, 0)
        .expect("register provider");
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
fn reject_unknown_provider() {
    let (sk, _vk) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_unknown".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_storage_receipt(&mut receipt, &sk);

    // Empty registry - provider not registered
    let registry = ProviderRegistry::new();
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

#[test]
fn reject_duplicate_receipt_across_blocks() {
    let (sk, vk) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_storage_receipt(&mut receipt, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");

    // First block: receipt accepted
    let mut nonce_tracker = NonceTracker::new(100);
    let result1 = validate_receipt(
        &Receipt::Storage(receipt.clone()),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(result1.is_ok());

    // Second block: same nonce replayed (replay attack)
    let result2 = validate_receipt(
        &Receipt::Storage(receipt.clone()),
        101,
        &registry,
        &mut nonce_tracker,
    );
    assert!(matches!(
        result2,
        Err(ValidationError::ReplayedNonce { .. })
    ));
}

#[test]
fn reject_corrupted_signature() {
    let (sk, vk) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_storage_receipt(&mut receipt, &sk);

    // Corrupt signature
    receipt.provider_signature[0] ^= 0xFF;

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");
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
fn accept_valid_storage_receipt() {
    let (sk, vk) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_storage_receipt(&mut receipt, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);

    let result = validate_receipt(
        &Receipt::Storage(receipt),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(result.is_ok());
}

#[test]
fn accept_valid_compute_receipt() {
    let (sk, vk) = create_test_keypair();

    let mut receipt = ComputeReceipt {
        job_id: "job_001".into(),
        provider: "provider_001".into(),
        compute_units: 1000,
        payment: 250,
        block_height: 100,
        verified: true,
        blocktorch: Some(sample_blocktorch_metadata("accept_valid")),
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_compute_receipt(&mut receipt, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);

    let result = validate_receipt(
        &Receipt::Compute(receipt),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(result.is_ok());
}

#[test]
fn reject_compute_receipt_with_zero_proof_latency() {
    let (sk, vk) = create_test_keypair();

    let mut meta = sample_blocktorch_metadata("zero_latency");
    meta.proof_latency_ms = 0;

    let mut receipt = ComputeReceipt {
        job_id: "job_004".into(),
        provider: "provider_004".into(),
        compute_units: 1000,
        payment: 250,
        block_height: 100,
        verified: true,
        blocktorch: Some(meta),
        provider_signature: vec![],
        signature_nonce: 4,
    };

    sign_compute_receipt(&mut receipt, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_004".into(), vk, 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);

    let result = validate_receipt(
        &Receipt::Compute(receipt),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(matches!(
        result,
        Err(ValidationError::InvalidBlockTorchMetadata { .. })
    ));
}

#[test]
fn receipt_deduplication_registry() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![1, 2, 3],
        signature_nonce: 1,
    });

    let id = ReceiptId::from_receipt(&receipt);
    let mut registry = ReceiptRegistry::new();

    // First registration succeeds
    assert!(registry.register(id).is_ok());

    // Duplicate registration fails
    assert!(matches!(
        registry.register(id),
        Err(ValidationError::DuplicateReceipt)
    ));
}

#[test]
fn nonce_prevents_replay_across_multiple_receipts() {
    let (sk, vk) = create_test_keypair();

    let mut receipt1 = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    let mut receipt2 = StorageReceipt {
        contract_id: "contract_002".into(),
        provider: "provider_001".into(),
        bytes: 2_000_000,
        price: 1000,
        block_height: 100,
        provider_escrow: 20000,
        provider_signature: vec![],
        signature_nonce: 1, // SAME NONCE - replay attack
    };

    sign_storage_receipt(&mut receipt1, &sk);
    sign_storage_receipt(&mut receipt2, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);

    // First receipt validates
    let result1 = validate_receipt(
        &Receipt::Storage(receipt1),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(result1.is_ok());

    // Second receipt with same nonce fails
    let result2 = validate_receipt(
        &Receipt::Storage(receipt2),
        100,
        &registry,
        &mut nonce_tracker,
    );
    assert!(matches!(
        result2,
        Err(ValidationError::ReplayedNonce { .. })
    ));
}

#[test]
fn forged_settlement_data_rejected() {
    let (sk, vk) = create_test_keypair();

    let mut receipt = StorageReceipt {
        contract_id: "contract_001".into(),
        provider: "provider_001".into(),
        bytes: 1_000_000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
        provider_signature: vec![],
        signature_nonce: 1,
    };

    sign_storage_receipt(&mut receipt, &sk);

    // Attacker modifies settlement data after signing
    receipt.price = 5_000_000; // Inflate price 10,000x

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider("provider_001".into(), vk, 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);

    // Signature won't verify because price changed
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
