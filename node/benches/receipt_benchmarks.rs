use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use the_block::block_binary::{decode_receipts, encode_receipts};
use the_block::receipts::{AdReceipt, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt};

fn create_storage_receipt(id: u64) -> Receipt {
    Receipt::Storage(StorageReceipt {
        contract_id: format!("storage_contract_{}", id),
        provider: format!("provider_{}", id),
        bytes: 1024 * 1024, // 1MB
        price_ct: 1000,
        block_height: id,
        provider_escrow: 10000,
    })
}

fn create_compute_receipt(id: u64) -> Receipt {
    Receipt::Compute(ComputeReceipt {
        job_id: format!("job_{}", id),
        provider: format!("provider_{}", id),
        compute_units: 50000,
        payment_ct: 500,
        block_height: id,
        verified: true,
    })
}

fn create_energy_receipt(id: u64) -> Receipt {
    Receipt::Energy(EnergyReceipt {
        contract_id: format!("energy_{}", id),
        provider: format!("provider_{}", id),
        energy_units: 1000,
        price_ct: 800,
        block_height: id,
        proof_hash: [0u8; 32],
    })
}

fn create_ad_receipt(id: u64) -> Receipt {
    Receipt::Ad(AdReceipt {
        campaign_id: format!("campaign_{}", id),
        publisher: format!("publisher_{}", id),
        impressions: 10000,
        spend_ct: 200,
        block_height: id,
        conversions: 100,
    })
}

fn bench_receipt_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_encoding");

    for size in [1, 10, 100, 1000, 10000].iter() {
        // Mixed receipt types (realistic scenario)
        let receipts: Vec<Receipt> = (0..*size)
            .map(|i| match i % 4 {
                0 => create_storage_receipt(i),
                1 => create_compute_receipt(i),
                2 => create_energy_receipt(i),
                _ => create_ad_receipt(i),
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("mixed", size), &receipts, |b, receipts| {
            b.iter(|| {
                let encoded = encode_receipts(black_box(receipts)).unwrap();
                black_box(encoded);
            });
        });

        // Storage-only receipts (best case)
        let storage_receipts: Vec<Receipt> = (0..*size).map(create_storage_receipt).collect();

        group.bench_with_input(
            BenchmarkId::new("storage_only", size),
            &storage_receipts,
            |b, receipts| {
                b.iter(|| {
                    let encoded = encode_receipts(black_box(receipts)).unwrap();
                    black_box(encoded);
                });
            },
        );
    }

    group.finish();
}

fn bench_receipt_decoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_decoding");

    for size in [1, 10, 100, 1000, 10000].iter() {
        let receipts: Vec<Receipt> = (0..*size)
            .map(|i| match i % 4 {
                0 => create_storage_receipt(i),
                1 => create_compute_receipt(i),
                2 => create_energy_receipt(i),
                _ => create_ad_receipt(i),
            })
            .collect();

        let encoded = encode_receipts(&receipts).unwrap();

        group.bench_with_input(BenchmarkId::new("mixed", size), &encoded, |b, encoded| {
            b.iter(|| {
                let decoded = decode_receipts(black_box(encoded)).unwrap();
                black_box(decoded);
            });
        });
    }

    group.finish();
}

fn bench_receipt_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_roundtrip");

    for size in [1, 10, 100, 1000, 10000].iter() {
        let receipts: Vec<Receipt> = (0..*size)
            .map(|i| match i % 4 {
                0 => create_storage_receipt(i),
                1 => create_compute_receipt(i),
                2 => create_energy_receipt(i),
                _ => create_ad_receipt(i),
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &receipts,
            |b, receipts| {
                b.iter(|| {
                    let encoded = encode_receipts(black_box(receipts)).unwrap();
                    let decoded = decode_receipts(black_box(&encoded)).unwrap();
                    black_box(decoded);
                });
            },
        );
    }

    group.finish();
}

fn bench_receipt_validation(c: &mut Criterion) {
    use the_block::receipts_validation::validate_receipt;

    let mut group = c.benchmark_group("receipt_validation");

    let receipt = create_storage_receipt(100);

    group.bench_function("validate_single", |b| {
        b.iter(|| {
            let result = validate_receipt(black_box(&receipt), black_box(100));
            black_box(result);
        });
    });

    // Validate many receipts
    for size in [10, 100, 1000, 10000].iter() {
        let receipts: Vec<Receipt> = (0..*size).map(|i| create_storage_receipt(i)).collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &receipts,
            |b, receipts| {
                b.iter(|| {
                    for (i, receipt) in receipts.iter().enumerate() {
                        let result = validate_receipt(black_box(receipt), black_box(i as u64));
                        black_box(result);
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_receipt_encoding,
    bench_receipt_decoding,
    bench_receipt_roundtrip,
    bench_receipt_validation
);
criterion_main!(benches);
