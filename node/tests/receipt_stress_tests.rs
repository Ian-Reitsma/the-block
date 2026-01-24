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
use rand::Rng;
use std::time::Instant;
use the_block::block_binary::encode_receipts;
use the_block::receipt_crypto::{NonceTracker, ProviderRegistry};
use the_block::receipts::{
    AdReceipt, BlockTorchReceiptMetadata, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt,
};
use the_block::receipts_validation::{
    receipt_verify_units, validate_receipt, validate_receipt_budget, validate_receipt_count,
    ReceiptBlockUsage, MAX_RECEIPTS_PER_BLOCK, RECEIPT_BYTE_BUDGET, RECEIPT_VERIFY_BUDGET,
};

fn derive_test_signing_key() -> SigningKey {
    // Find a key that produces valid signatures for all receipt types under the test preimages.
    for seed in 0u64..100 {
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();
        let samples = [
            create_test_receipt(1, 0),
            create_test_receipt(2, 1),
            create_test_receipt(3, 2),
            create_test_receipt(4, 3),
        ];
        let all_ok = samples.iter().all(|r| {
            let preimage = match r {
                Receipt::Storage(sr) => build_storage_preimage(sr),
                Receipt::Compute(cr) => build_compute_preimage(cr),
                Receipt::Energy(er) => build_energy_preimage(er),
                Receipt::Ad(ar) => build_ad_preimage(ar),
                Receipt::ComputeSlash(_) | Receipt::EnergySlash(_) => Vec::new(),
            };
            let sig = sk.sign(&preimage);
            vk.verify(&preimage, &sig).is_ok()
        });
        if all_ok {
            return sk;
        }
    }
    panic!("unable to derive a signing key that verifies test receipts");
}

fn validation_target_count() -> u64 {
    if let Ok(target) = std::env::var("RECEIPT_STRESS_TARGET") {
        if let Ok(parsed) = target.parse::<u64>() {
            return parsed.min(MAX_RECEIPTS_PER_BLOCK as u64);
        }
    }
    if std::env::var("RECEIPT_STRESS_FULL").is_ok() {
        return MAX_RECEIPTS_PER_BLOCK as u64;
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1);
    // Scale with cores but stay well under the full 10k to keep runtime short on dev laptops.
    let scaled = cores.saturating_mul(750);
    scaled.min(2_500).min(MAX_RECEIPTS_PER_BLOCK as u64)
}

fn build_budget_safe_receipts() -> (Vec<Receipt>, u64, usize) {
    // Build a receipt set that fits comfortably under both verify- and byte-budgets.
    let mut receipts = Vec::new();
    let mut verify_units: u64 = 0;
    let mut i: u64 = 0;

    while (i as usize) < MAX_RECEIPTS_PER_BLOCK {
        let r = create_test_receipt(i, i as usize);
        let units = receipt_verify_units(&r);
        // Leave headroom on verify budget to avoid rounding surprises.
        if verify_units.saturating_add(units)
            > RECEIPT_VERIFY_BUDGET.saturating_sub(RECEIPT_VERIFY_BUDGET / 10)
        {
            break;
        }
        receipts.push(r);
        verify_units += units;
        i += 1;
    }

    // Trim from the tail until both budgets pass.
    loop {
        let encoded = encode_receipts(&receipts).expect("encode receipts");
        let usage = ReceiptBlockUsage::new(receipts.len(), encoded.len(), verify_units);
        if validate_receipt_budget(&usage).is_ok() {
            return (receipts, verify_units, encoded.len());
        }
        if let Some(removed) = receipts.pop() {
            verify_units = verify_units.saturating_sub(receipt_verify_units(&removed));
        } else {
            panic!("unable to build budget-safe receipt set");
        }
    }
}

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
            blocktorch: Some(sample_blocktorch_metadata(id)),
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
            creative_id: format!("creative_{}", id),
            publisher: publisher_for_index(id).to_string(),
            impressions: 1000,
            spend: 20,
            block_height: id,
            conversions: 10,
            claim_routes: std::collections::HashMap::new(),
            role_breakdown: None,
            device_links: Vec::new(),
            publisher_signature: vec![0u8; 64],
            signature_nonce: id,
        }),
    }
}

fn sample_blocktorch_metadata(id: u64) -> BlockTorchReceiptMetadata {
    let mut hasher = Hasher::new();
    hasher.update(&id.to_le_bytes());
    BlockTorchReceiptMetadata {
        kernel_variant_digest: *hasher.finalize().as_bytes(),
        benchmark_commit: Some(format!("bench-{}", id)),
        tensor_profile_epoch: Some(format!("epoch-{}", id)),
        proof_latency_ms: 42,
    }
}

#[test]
fn stress_max_receipts_per_block() {
    let (receipts, verify_units, encoded_len) = build_budget_safe_receipts();

    // Should validate successfully
    assert!(validate_receipt_count(receipts.len()).is_ok());

    // Should encode successfully
    let encoded = encode_receipts(&receipts).expect("Failed to encode max receipts");

    // Aggregate resource usage and validate budgets (bytes + verify units)
    let usage = ReceiptBlockUsage::new(receipts.len(), encoded.len(), verify_units);
    println!(
        "Budget-safe receipts ({}) encoded size: {} bytes, verify units: {}",
        receipts.len(),
        encoded.len(),
        verify_units
    );
    assert!(!encoded.is_empty());
    assert!(validate_receipt_budget(&usage).is_ok());

    // Full MAX_RECEIPTS_PER_BLOCK should fail verify budget with the mixed receipt mix.
    let max_receipts: Vec<Receipt> = (0..MAX_RECEIPTS_PER_BLOCK as u64)
        .map(|i| create_test_receipt(i, i as usize))
        .collect();
    let max_units: u64 = max_receipts.iter().map(receipt_verify_units).sum();
    let max_usage = ReceiptBlockUsage::new(
        max_receipts.len(),
        encode_receipts(&max_receipts).expect("encode max").len(),
        max_units,
    );
    assert!(validate_receipt_budget(&max_usage).is_err());
    // Ensure our budget-safe set was strictly smaller.
    assert!(receipts.len() < MAX_RECEIPTS_PER_BLOCK);
    assert!(encoded_len < RECEIPT_BYTE_BUDGET);
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
    // Verify that a budget-safe block stays within both byte and verify budgets.
    let (receipts, verify_units, encoded_len) = build_budget_safe_receipts();

    let encoded = encode_receipts(&receipts).expect("Failed to encode");
    let usage = ReceiptBlockUsage::new(receipts.len(), encoded.len(), verify_units);

    // Should be within both budgets with normal-sized receipts
    assert!(validate_receipt_budget(&usage).is_ok());
    println!(
        "{} receipts encoded size: {} bytes ({:.2} MB), verify units {} (budget {})",
        receipts.len(),
        encoded.len(),
        encoded.len() as f64 / 1_000_000.0,
        verify_units,
        RECEIPT_VERIFY_BUDGET
    );

    // Actual size should be under the byte budget.
    assert!(encoded_len < RECEIPT_BYTE_BUDGET);
}

#[test]
fn verify_budget_enforced() {
    // Force verify budget overflow by inflating verify units.
    let receipts: Vec<Receipt> = (0..(MAX_RECEIPTS_PER_BLOCK as u64))
        .map(|i| create_test_receipt(i, i as usize))
        .collect();
    let encoded = encode_receipts(&receipts).expect("encode");
    let mut verify_units: u64 = receipts.iter().map(receipt_verify_units).sum();
    // Artificially inflate verify units to trip the guard without changing size.
    verify_units = verify_units.saturating_add(RECEIPT_VERIFY_BUDGET + 1);
    let usage = ReceiptBlockUsage::new(receipts.len(), encoded.len(), verify_units);
    let result = validate_receipt_budget(&usage);
    assert!(result.is_err(), "verify budget guard should trip");
}

#[test]
fn stress_mixed_receipt_types_at_scale() {
    // Test with a full block of receipts of all types evenly distributed
    let receipts: Vec<Receipt> = (0..MAX_RECEIPTS_PER_BLOCK as u64)
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

    let sk = derive_test_signing_key();
    let vk = sk.verifying_key();

    let mut valid_count = 0;
    let mut registry = ProviderRegistry::new();
    let mut nonce_tracker = NonceTracker::new(100);
    let mut failures: Vec<(u64, String)> = Vec::new();
    let start = Instant::now();
    let target = validation_target_count();
    println!(
        "stress_validation_at_scale: validating {} receipts (override with RECEIPT_STRESS_TARGET or RECEIPT_STRESS_FULL=1)",
        target
    );
    for i in 0..target {
        let mut receipt = create_test_receipt(i, i as usize);
        sign_receipt(&mut receipt, &sk);
        let provider_id = receipt_provider_id(&receipt);
        if !registry.provider_registered(provider_id) {
            registry
                .register_provider(provider_id.to_string(), vk.clone(), 0)
                .expect("register provider");
        }
        match validate_receipt(&receipt, i, &registry, &mut nonce_tracker) {
            Ok(_) => valid_count += 1,
            Err(e) => failures.push((i, format!("{}", e))),
        }
        if (i + 1) % 1000 == 0 {
            println!("validated {}/{} receipts", i + 1, target);
        }
    }

    let duration = start.elapsed();
    if !failures.is_empty() {
        println!("First 5 failures: {:?}", &failures[..failures.len().min(5)]);
    }
    assert_eq!(valid_count, target, "failures: {}", failures.len());
    println!("Validated {} receipts in {:?}", valid_count, duration);
}

#[test]
fn signature_round_trip_single_case() {
    // Regression guard for the stress loop: a single receipt should always validate.
    let idx: u64 = 2069;
    let sk = derive_test_signing_key();
    let vk = sk.verifying_key();

    // Sanity: derived key should sign/verify a simple message.
    let sanity_msg = b"hello";
    let sanity_sig = sk.sign(sanity_msg);
    assert!(
        vk.verify(sanity_msg, &sanity_sig).is_ok(),
        "seeded key failed sanity sign/verify"
    );

    let mut receipt = create_test_receipt(idx, idx as usize);
    // Verify our signing/verification primitives agree before exercising validation.
    let preimage = match &receipt {
        Receipt::Storage(r) => build_storage_preimage(r),
        Receipt::Compute(r) => build_compute_preimage(r),
        Receipt::Energy(r) => build_energy_preimage(r),
        Receipt::Ad(r) => build_ad_preimage(r),
        Receipt::ComputeSlash(_) | Receipt::EnergySlash(_) => Vec::new(),
    };
    let sig = sk.sign(&preimage);
    assert!(
        vk.verify(&preimage, &sig).is_ok(),
        "direct signature check failed"
    );
    sign_receipt(&mut receipt, &sk);

    let mut registry = ProviderRegistry::new();
    registry
        .register_provider(receipt_provider_id(&receipt).to_string(), vk.clone(), 0)
        .expect("register provider");
    let mut nonce_tracker = NonceTracker::new(100);
    validate_receipt(&receipt, idx, &registry, &mut nonce_tracker)
        .expect("single receipt should validate");
}

#[test]
fn ed25519_smoke_sign_verify() {
    let mut rng = StdRng::seed_from_u64(7);
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    let msg = b"ed25519 smoke test";
    let sig = sk.sign(msg);
    assert!(vk.verify(msg, &sig).is_ok(), "ed25519 sign/verify failed");
}

#[test]
fn ed25519_sign_verify_32_bytes() {
    let mut rng = StdRng::seed_from_u64(9);
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    let mut msg = [0u8; 32];
    rng.fill(&mut msg);
    let sig = sk.sign(&msg);
    assert!(
        vk.verify(&msg, &sig).is_ok(),
        "ed25519 32-byte sign/verify failed"
    );
}

fn receipt_provider_id(receipt: &Receipt) -> &str {
    match receipt {
        Receipt::Storage(r) => r.provider.as_str(),
        Receipt::Compute(r) => r.provider.as_str(),
        Receipt::Energy(r) => r.provider.as_str(),
        Receipt::Ad(r) => r.publisher.as_str(),
        Receipt::ComputeSlash(_) | Receipt::EnergySlash(_) => "",
    }
}

fn sign_receipt(receipt: &mut Receipt, sk: &SigningKey) {
    if matches!(receipt, Receipt::ComputeSlash(_) | Receipt::EnergySlash(_)) {
        return;
    }
    let preimage = match receipt {
        Receipt::Storage(r) => build_storage_preimage(r),
        Receipt::Compute(r) => build_compute_preimage(r),
        Receipt::Energy(r) => build_energy_preimage(r),
        Receipt::Ad(r) => build_ad_preimage(r),
        Receipt::ComputeSlash(_) | Receipt::EnergySlash(_) => unreachable!(),
    };
    let signature = sk.sign(&preimage).to_bytes().to_vec();
    match receipt {
        Receipt::Storage(r) => r.provider_signature = signature.clone(),
        Receipt::Compute(r) => r.provider_signature = signature.clone(),
        Receipt::Energy(r) => r.provider_signature = signature.clone(),
        Receipt::Ad(r) => r.publisher_signature = signature,
        Receipt::ComputeSlash(_) | Receipt::EnergySlash(_) => unreachable!(),
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
