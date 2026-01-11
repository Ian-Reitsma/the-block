//! Stress tests for receipt system
//!
//! Tests system behavior under extreme conditions:
//! - Maximum receipt counts (10,000 per block)
//! - Maximum receipt sizes
//! - Validation at scale
//! - Memory pressure

use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::StdRng;
use std::time::Instant;
use the_block::block_binary::encode_receipts;
use the_block::receipt_crypto::{NonceTracker, ProviderRegistry};
use the_block::receipts::{AdReceipt, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt};
use the_block::receipts_validation::{
    validate_receipt, validate_receipt_count, validate_receipt_size, MAX_RECEIPTS_PER_BLOCK,
};

const RECEIPT_PROVIDER_POOL: [&str; 4] = [
    "stress-provider-0",
    "stress-provider-1",
    "stress-provider-2",
    "stress-provider-3",
];

const RECEIPT_PUBLISHER_POOL: [&str; 4] = [
    "stress-publisher-0",
    "stress-publisher-1",
    "stress-publisher-2",
    "stress-publisher-3",
];

fn provider_for_index(id: u64) -> &'static str {
    RECEIPT_PROVIDER_POOL[(id as usize) % RECEIPT_PROVIDER_POOL.len()]
}

fn publisher_for_index(id: u64) -> &'static str {
    RECEIPT_PUBLISHER_POOL[(id as usize) % RECEIPT_PUBLISHER_POOL.len()]
}

fn create_test_receipt(id: u64, receipt_type: usize) -> Receipt {
    match receipt_type % 4 {
        0 => Receipt::Storage(StorageReceipt {
            contract_id: format!("sc_{}", id),
            provider: provider_for_index(id).to_string(),
            bytes: 1024,
            price: 100,
            block_height: id,
            provider_escrow: 1000,
            provider_signature: vec![0u8; 64],
            signature_nonce: id,
        }),
        1 => Receipt::Compute(ComputeReceipt {
            job_id: format!("job_{}", id),
            provider: provider_for_index(id).to_string(),
            compute_units: 1000,
            payment: 50,
            block_height: id,
            verified: true,
            provider_signature: vec![0u8; 64],
            signature_nonce: id,
        }),
        2 => Receipt::Energy(EnergyReceipt {
            contract_id: format!("energy_{}", id),
            provider: provider_for_index(id).to_string(),
            energy_units: 500,
            price: 75,
            block_height: id,
            proof_hash: [0u8; 32],
            provider_signature: vec![0u8; 64],
            signature_nonce: id,
        }),
        _ => Receipt::Ad(AdReceipt {
            campaign_id: format!("campaign_{}", id),
            publisher: publisher_for_index(id).to_string(),
            impressions: 1000,
            spend: 20,
            block_height: id,
            conversions: 10,
            publisher_signature: vec![0u8; 64],
            signature_nonce: id,
        }),
    }
}

#[test]
fn stress_max_receipts_per_block() {
    // Test with exactly the maximum allowed receipts
    let receipts: Vec<Receipt> = (0..MAX_RECEIPTS_PER_BLOCK as u64)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    // Should validate successfully
    assert!(validate_receipt_count(receipts.len()).is_ok());

    // Should encode successfully
    let encoded = encode_receipts(&receipts).expect("Failed to encode max receipts");

    // Verify encoded size is reasonable
    println!(
        "Max receipts ({}) encoded size: {} bytes",
        receipts.len(),
        encoded.len()
    );
    assert!(!encoded.is_empty());
    assert!(validate_receipt_size(encoded.len()).is_ok());
}

#[test]
fn stress_exceeds_max_receipts() {
    // Test with more than maximum allowed
    let count = MAX_RECEIPTS_PER_BLOCK + 1;
    let result = validate_receipt_count(count);
    assert!(result.is_err());
}

#[test]
fn stress_large_receipt_payload() {
    // Create receipts with very long string fields (near max)
    let long_id = "a".repeat(250); // Near MAX_STRING_FIELD_LENGTH

    let receipts: Vec<Receipt> = (0..1000)
        .map(|i| {
            Receipt::Storage(StorageReceipt {
                contract_id: format!("{}_{}", long_id, i),
                provider: format!("{}_{}", long_id, i),
                bytes: u64::MAX / 2,
                price: u64::MAX / 2,
                block_height: i,
                provider_escrow: u64::MAX / 2,
                provider_signature: vec![0u8; 64],
                signature_nonce: i,
            })
        })
        .collect();

    let encoded = encode_receipts(&receipts).expect("Failed to encode large receipts");
    println!(
        "Large receipts ({}) encoded size: {} bytes",
        receipts.len(),
        encoded.len()
    );
    assert!(!encoded.is_empty());
}

#[test]
fn stress_encoding_overhead_within_limit() {
    // Verify that 10,000 receipts encoded size stays under 10MB limit
    let receipts: Vec<Receipt> = (0..10_000)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    let encoded = encode_receipts(&receipts).expect("Failed to encode");

    // Should be well under the limit with normal-sized receipts
    assert!(validate_receipt_size(encoded.len()).is_ok());
    println!(
        "10,000 receipts encoded size: {} bytes ({:.2} MB)",
        encoded.len(),
        encoded.len() as f64 / 1_000_000.0
    );

    // Actual size should be reasonable (< 5MB for 10k receipts, well under the 10MB limit)
    assert!(
        encoded.len() < 5_000_000,
        "Encoded size unexpectedly large: {}",
        encoded.len()
    );
}

#[test]
fn stress_mixed_receipt_types_at_scale() {
    // Test with 10,000 receipts of all types evenly distributed
    let receipts: Vec<Receipt> = (0..10_000)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    // Count each type
    let storage_count = receipts
        .iter()
        .filter(|r| matches!(r, Receipt::Storage(_)))
        .count();
    let compute_count = receipts
        .iter()
        .filter(|r| matches!(r, Receipt::Compute(_)))
        .count();
    let energy_count = receipts
        .iter()
        .filter(|r| matches!(r, Receipt::Energy(_)))
        .count();
    let ad_count = receipts
        .iter()
        .filter(|r| matches!(r, Receipt::Ad(_)))
        .count();

    println!("Type distribution:");
    println!("  Storage: {}", storage_count);
    println!("  Compute: {}", compute_count);
    println!("  Energy: {}", energy_count);
    println!("  Ad: {}", ad_count);

    // Should be roughly equal distribution
    assert!(storage_count > 2000 && storage_count < 3000);
    assert!(compute_count > 2000 && compute_count < 3000);
    assert!(energy_count > 2000 && energy_count < 3000);
    assert!(ad_count > 2000 && ad_count < 3000);

    // Should encode successfully
    let encoded = encode_receipts(&receipts).expect("Failed to encode mixed types");
    println!("Mixed types encoded size: {} bytes", encoded.len());
    assert!(!encoded.is_empty());
}

#[test]
fn stress_validation_at_scale() {
    if cfg!(debug_assertions) {
        println!(
            "Skipping stress_validation_at_scale in debug builds; run `cargo test --release` for the full suite"
        );
        return;
    }

    let mut rng = StdRng::seed_from_u64(42);
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();

    let mut valid_count = 0;
    let mut registry = ProviderRegistry::new();
    let mut nonce_tracker = NonceTracker::new(100);
    let start = Instant::now();
    for i in 0..MAX_RECEIPTS_PER_BLOCK as u64 {
        let mut receipt = create_test_receipt(i, i as usize);
        sign_receipt(&mut receipt, &sk);
        let provider_id = receipt_provider_id(&receipt);
        if !registry.provider_registered(provider_id) {
            registry
                .register_provider(provider_id.to_string(), vk.clone(), 0)
                .expect("register provider");
        }
        if validate_receipt(&receipt, i, &registry, &mut nonce_tracker).is_ok() {
            valid_count += 1;
        }
        if (i + 1) % 1000 == 0 {
            println!("validated {}/{} receipts", i + 1, MAX_RECEIPTS_PER_BLOCK);
        }
    }

    let duration = start.elapsed();
    assert_eq!(valid_count, MAX_RECEIPTS_PER_BLOCK);
    println!("Validated {} receipts in {:?}", valid_count, duration);
}

fn receipt_provider_id(receipt: &Receipt) -> &str {
    match receipt {
        Receipt::Storage(r) => r.provider.as_str(),
        Receipt::Compute(r) => r.provider.as_str(),
        Receipt::Energy(r) => r.provider.as_str(),
        Receipt::Ad(r) => r.publisher.as_str(),
    }
}

fn sign_receipt(receipt: &mut Receipt, sk: &SigningKey) {
    let preimage = match receipt {
        Receipt::Storage(r) => build_storage_preimage(r),
        Receipt::Compute(r) => build_compute_preimage(r),
        Receipt::Energy(r) => build_energy_preimage(r),
        Receipt::Ad(r) => build_ad_preimage(r),
    };
    let signature = sk.sign(&preimage).to_bytes().to_vec();
    match receipt {
        Receipt::Storage(r) => r.provider_signature = signature.clone(),
        Receipt::Compute(r) => r.provider_signature = signature.clone(),
        Receipt::Energy(r) => r.provider_signature = signature.clone(),
        Receipt::Ad(r) => r.publisher_signature = signature,
    }
}

fn build_storage_preimage(receipt: &StorageReceipt) -> Vec<u8> {
    let mut hasher = Hasher::new();
    hasher.update(b"storage");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.contract_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.bytes.to_le_bytes());
    hasher.update(&receipt.price.to_le_bytes());
    hasher.update(&receipt.provider_escrow.to_le_bytes());
    hasher.update(&receipt.signature_nonce.to_le_bytes());
    hasher.finalize().as_bytes().to_vec()
}

fn build_compute_preimage(receipt: &ComputeReceipt) -> Vec<u8> {
    let mut hasher = Hasher::new();
    hasher.update(b"compute");
    hasher.update(&receipt.block_height.to_le_bytes());
    hasher.update(receipt.job_id.as_bytes());
    hasher.update(receipt.provider.as_bytes());
    hasher.update(&receipt.compute_units.to_le_bytes());
    hasher.update(&receipt.payment.to_le_bytes());
    hasher.update(&[u8::from(receipt.verified)]);
    hasher.update(&receipt.signature_nonce.to_le_bytes());
    hasher.finalize().as_bytes().to_vec()
}

fn build_energy_preimage(receipt: &EnergyReceipt) -> Vec<u8> {
    let mut hasher = Hasher::new();
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

fn build_ad_preimage(receipt: &AdReceipt) -> Vec<u8> {
    let mut hasher = Hasher::new();
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

#[test]
fn stress_empty_receipts() {
    // Test with zero receipts (edge case)
    let receipts: Vec<Receipt> = vec![];

    assert!(validate_receipt_count(receipts.len()).is_ok());

    let encoded = encode_receipts(&receipts).expect("Failed to encode empty");
    // Empty receipts still have encoding overhead
    assert!(
        encoded.len() < 100,
        "Empty encoding overhead too large: {}",
        encoded.len()
    );
}

#[test]
fn stress_single_receipt() {
    // Test with exactly one receipt (edge case)
    let receipts = vec![create_test_receipt(0, 0)];

    assert!(validate_receipt_count(receipts.len()).is_ok());

    let encoded = encode_receipts(&receipts).expect("Failed to encode single");
    assert!(!encoded.is_empty());
}

#[test]
fn stress_all_storage_receipts() {
    // Test homogeneous receipt type
    let receipts: Vec<Receipt> = (0..5000)
        .map(|i| {
            Receipt::Storage(StorageReceipt {
                contract_id: format!("contract_{}", i),
                provider: format!("provider_{}", i),
                bytes: i * 1024,
                price: i * 10,
                block_height: i,
                provider_escrow: i * 100,
                provider_signature: vec![0u8; 64],
                signature_nonce: i,
            })
        })
        .collect();

    let encoded = encode_receipts(&receipts).expect("Failed to encode storage");
    println!(
        "5000 storage receipts encoded size: {} bytes",
        encoded.len()
    );
    assert!(!encoded.is_empty());
}

#[test]
fn stress_memory_efficiency() {
    // Test that we can create and process 10k receipts without OOM
    let receipts: Vec<Receipt> = (0..10_000)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    // Encode
    let encoded = encode_receipts(&receipts).expect("Failed to encode");

    // Measure memory usage (approx)
    let receipt_memory = receipts.len() * std::mem::size_of::<Receipt>();
    let encoded_memory = encoded.len();

    println!("Memory usage:");
    println!("  Receipt structs: ~{} bytes", receipt_memory);
    println!("  Encoded bytes: {} bytes", encoded_memory);
    println!(
        "  Ratio: {:.2}x",
        receipt_memory as f64 / encoded_memory as f64
    );

    // Serialized size is reasonable (within 3x of in-memory due to encoding overhead and string data)
    assert!(
        encoded_memory < receipt_memory * 3,
        "Encoded too large: {} vs {}",
        encoded_memory,
        receipt_memory
    );
}

/// Performance regression test - encoding 1000 receipts should be fast
#[test]
fn stress_encoding_performance() {
    use std::time::Instant;

    let receipts: Vec<Receipt> = (0..1000)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    let start = Instant::now();
    let encoded = encode_receipts(&receipts).expect("Failed to encode");
    let duration = start.elapsed();

    println!(
        "Encoded 1000 receipts in {:?} ({} bytes)",
        duration,
        encoded.len()
    );

    // Should encode in under 100ms (very conservative)
    assert!(
        duration.as_millis() < 100,
        "Encoding too slow: {:?}",
        duration
    );
}

#[test]
fn stress_validation_performance() {
    use std::time::Instant;

    let receipts: Vec<Receipt> = (0..10_000)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();

    let start = Instant::now();
    let registry = ProviderRegistry::new();
    let mut nonce_tracker = NonceTracker::new(100);
    for (i, receipt) in receipts.iter().enumerate() {
        let _ = validate_receipt(receipt, i as u64, &registry, &mut nonce_tracker);
    }
    let duration = start.elapsed();

    println!("Validated 10,000 receipts in {:?}", duration);

    // Should validate in under 100ms (very conservative)
    assert!(
        duration.as_millis() < 100,
        "Validation too slow: {:?}",
        duration
    );
}
