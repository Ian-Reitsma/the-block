use std::hint::black_box;
use std::sync::OnceLock;

use crypto_suite::hashing::blake3::Hasher;
use testkit::bench;

use the_block::{
    block_binary::{decode_receipts, encode_receipts},
    receipt_crypto::{NonceTracker, ProviderRegistry},
    receipts::{
        AdReceipt, BlockTorchReceiptMetadata, ComputeReceipt, EnergyReceipt, Receipt,
        StorageReceipt,
    },
    receipts_validation::validate_receipt,
};

const ENCODING_SIZES: [usize; 5] = [1, 10, 100, 1_000, 10_000];
const VALIDATION_SIZES: [usize; 4] = [10, 100, 1_000, 10_000];
const MAX_RECEIPT_POOL_SIZE: usize = 10_000;

fn create_storage_receipt(id: u64) -> Receipt {
    Receipt::Storage(StorageReceipt {
        contract_id: format!("storage_contract_{}", id),
        provider: format!("provider_{}", id),
        bytes: 1024 * 1024, // 1MB
        price: 1_000,
        block_height: id,
        provider_escrow: 10_000,
        provider_signature: vec![0u8; 64],
        signature_nonce: id,
    })
}

fn create_compute_receipt(id: u64) -> Receipt {
    Receipt::Compute(ComputeReceipt {
        job_id: format!("job_{}", id),
        provider: format!("provider_{}", id),
        compute_units: 50_000,
        payment: 500,
        block_height: id,
        verified: true,
        blocktorch: Some(sample_blocktorch_metadata(id)),
        provider_signature: vec![0u8; 64],
        signature_nonce: id,
    })
}

fn sample_blocktorch_metadata(id: u64) -> BlockTorchReceiptMetadata {
    let mut hasher = Hasher::new();
    hasher.update(&id.to_le_bytes());
    BlockTorchReceiptMetadata {
        kernel_variant_digest: *hasher.finalize().as_bytes(),
        descriptor_digest: {
            let mut descriptor_hasher = Hasher::new();
            descriptor_hasher.update(b"descriptor");
            descriptor_hasher.update(&id.to_le_bytes());
            *descriptor_hasher.finalize().as_bytes()
        },
        output_digest: {
            let mut output_hasher = Hasher::new();
            output_hasher.update(b"output");
            output_hasher.update(&id.to_le_bytes());
            *output_hasher.finalize().as_bytes()
        },
        benchmark_commit: Some(format!("bench-{}", id)),
        tensor_profile_epoch: Some(format!("epoch-{}", id)),
        proof_latency_ms: 21,
    }
}

fn create_energy_receipt(id: u64) -> Receipt {
    Receipt::Energy(EnergyReceipt {
        contract_id: format!("energy_{}", id),
        provider: format!("provider_{}", id),
        energy_units: 1_000,
        price: 800,
        block_height: id,
        proof_hash: [0u8; 32],
        provider_signature: vec![0u8; 64],
        signature_nonce: id,
    })
}

fn create_ad_receipt(id: u64) -> Receipt {
    Receipt::Ad(AdReceipt {
        campaign_id: format!("campaign_{}", id),
        creative_id: format!("creative_{}", id),
        publisher: format!("publisher_{}", id),
        impressions: 10_000,
        spend: 200,
        block_height: id,
        conversions: 100,
        claim_routes: std::collections::HashMap::new(),
        role_breakdown: None,
        device_links: Vec::new(),
        publisher_signature: vec![0u8; 64],
        signature_nonce: id,
    })
}

fn mixed_pool() -> &'static [Receipt] {
    static POOL: OnceLock<Vec<Receipt>> = OnceLock::new();
    POOL.get_or_init(|| {
        (0..MAX_RECEIPT_POOL_SIZE)
            .map(|i| match i % 4 {
                0 => create_storage_receipt(i as u64),
                1 => create_compute_receipt(i as u64),
                2 => create_energy_receipt(i as u64),
                _ => create_ad_receipt(i as u64),
            })
            .collect()
    })
}

fn storage_pool() -> &'static [Receipt] {
    static POOL: OnceLock<Vec<Receipt>> = OnceLock::new();
    POOL.get_or_init(|| {
        (0..MAX_RECEIPT_POOL_SIZE)
            .map(|i| create_storage_receipt(i as u64))
            .collect()
    })
}

fn run_encoding_bench(name: &str, receipts: &[Receipt]) {
    bench::run(name, bench::DEFAULT_ITERATIONS, || {
        let encoded = encode_receipts(receipts).expect("encode receipts");
        black_box(encoded);
    });
}

fn run_decoding_bench(name: &str, encoded: &[u8]) {
    bench::run(name, bench::DEFAULT_ITERATIONS, || {
        let decoded = decode_receipts(black_box(encoded)).expect("decode receipts");
        black_box(decoded);
    });
}

fn run_roundtrip_bench(name: &str, receipts: &[Receipt]) {
    bench::run(name, bench::DEFAULT_ITERATIONS, || {
        let encoded = encode_receipts(receipts).expect("encode receipts");
        let decoded = decode_receipts(black_box(encoded.as_slice())).expect("decode receipts");
        black_box(decoded);
    });
}

fn run_validation_bench(name: &str, receipts: &[Receipt]) {
    bench::run(name, bench::DEFAULT_ITERATIONS, || {
        let registry = ProviderRegistry::new();
        let mut nonce_tracker = NonceTracker::new(100);
        for (idx, receipt) in receipts.iter().enumerate() {
            let result = validate_receipt(receipt, idx as u64, &registry, &mut nonce_tracker);
            let _ = black_box(result);
        }
    });
}

fn main() {
    for &size in ENCODING_SIZES.iter() {
        let mixed = &mixed_pool()[..size];
        let storage = &storage_pool()[..size];

        let name = format!("receipt_encoding_mixed_{size}");
        run_encoding_bench(&name, mixed);

        let name = format!("receipt_encoding_storage_{size}");
        run_encoding_bench(&name, storage);

        let encoded = encode_receipts(mixed).expect("encode receipts");
        let name = format!("receipt_decoding_mixed_{size}");
        run_decoding_bench(&name, &encoded);

        let encoded = encode_receipts(storage).expect("encode receipts");
        let name = format!("receipt_decoding_storage_{size}");
        run_decoding_bench(&name, &encoded);

        let name = format!("receipt_roundtrip_mixed_{size}");
        run_roundtrip_bench(&name, mixed);
    }

    let single = &storage_pool()[..1];
    run_validation_bench("receipt_validation_single", single);

    for &size in VALIDATION_SIZES.iter() {
        let receipts = &storage_pool()[..size];
        let name = format!("receipt_validation_storage_{size}");
        run_validation_bench(&name, receipts);
    }
}
