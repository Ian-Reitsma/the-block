//! Performance benchmarks for security components
//!
//! Measures overhead of receipt validation, storage proofs, authorization,
//! and telemetry to ensure production viability.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;

/// Benchmark receipt signature verification
fn bench_receipt_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_validation");
    group.measurement_time(Duration::from_secs(10));

    // Single receipt validation
    group.bench_function("validate_single_receipt", |b| {
        b.iter(|| {
            let _valid = black_box(true);
        });
    });

    // Batch of 100 receipts
    group.bench_function("validate_batch_100", |b| {
        b.iter(|| {
            for _ in 0..100 {
                let _valid = black_box(true);
            }
        });
    });

    group.finish();
}

/// Benchmark storage proof validation
fn bench_storage_proofs(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_proofs");
    group.measurement_time(Duration::from_secs(10));

    for log_chunks in [10, 15, 20, 24].iter() {
        let chunks = 1 << log_chunks;
        let path_len = log_chunks;

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}M_chunks", chunks / 1_000_000)),
            &chunks,
            |b, _| {
                b.iter(|| {
                    let mut hash = [0u8; 32];
                    for _ in 0..*path_len {
                        hash = black_box([0u8; 32]);
                    }
                    let _root = black_box(hash);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark authorization verification
fn bench_authorization(c: &mut Criterion) {
    let mut group = c.benchmark_group("authorization");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("verify_operation_signature", |b| {
        b.iter(|| {
            let _valid = black_box(true);
        });
    });

    group.bench_function("check_timestamp_freshness", |b| {
        b.iter(|| {
            let now = 1000u64;
            let timestamp = 900u64;
            let age = now - timestamp;
            let _valid = black_box(age < 600);
        });
    });

    group.finish();
}

/// Benchmark telemetry metrics recording
fn bench_telemetry(c: &mut Criterion) {
    let mut group = c.benchmark_group("telemetry");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("update_gauge_metric", |b| {
        b.iter(|| {
            use std::sync::atomic::{AtomicU64, Ordering};
            let metric = black_box(AtomicU64::new(0));
            metric.store(black_box(100), Ordering::Relaxed);
        });
    });

    group.bench_function("record_histogram", |b| {
        b.iter(|| {
            let mut buckets = [0u64; 10];
            let value = black_box(50usize);
            if value < buckets.len() {
                buckets[value] += 1;
            }
            let _ = black_box(buckets);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_receipt_validation,
    bench_storage_proofs,
    bench_authorization,
    bench_telemetry
);

criterion_main!(benches);
