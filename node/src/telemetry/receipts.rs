//! Receipt telemetry and metrics tracking
//!
//! Tracks receipt emission, serialization size, and derivation performance.
//! Enables monitoring of market activity at the consensus level.

#[cfg(feature = "telemetry")]
use super::{
    blocktorch_update_metadata, register_counter, register_gauge, register_histogram,
    register_int_gauge, register_int_gauge_vec,
};
use crate::receipts::Receipt;
use crate::receipts_validation::ReceiptBlockUsage;
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
use crypto_suite::hex;
#[cfg(feature = "telemetry")]
use runtime::telemetry::IntGaugeVec;
#[cfg(feature = "telemetry")]
use runtime::telemetry::{Gauge, Histogram, IntCounter, IntGauge};
#[cfg(feature = "telemetry")]
use std::time::Duration;

/// Receipt count by market type (telemetry)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_STORAGE: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipts_storage_total",
        "Total storage receipts across all blocks",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_COMPUTE: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipts_compute_total",
        "Total compute receipts across all blocks",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_COMPUTE_SLASH: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipts_compute_slash_total",
        "Total compute SLA slash receipts across all blocks",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_ENERGY: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipts_energy_total",
        "Total energy receipts across all blocks",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_AD: Lazy<IntCounter> =
    Lazy::new(|| register_counter("receipts_ad_total", "Total ad receipts across all blocks"));

/// Receipts in current block (gauge - resets per block)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_PER_BLOCK: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge("receipts_per_block", "Number of receipts in current block"));

/// Storage receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_STORAGE_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_storage_per_block",
        "Number of storage receipts in current block",
    )
});

/// Compute receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_COMPUTE_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_compute_per_block",
        "Number of compute receipts in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_COMPUTE_SLASH_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_compute_slash_per_block",
        "Number of compute SLA slash receipts in current block",
    )
});

/// Energy receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_ENERGY_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_energy_per_block",
        "Number of energy receipts in current block",
    )
});

/// Slashed energy receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_ENERGY_SLASH_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_energy_slash_per_block",
        "Number of energy slash receipts in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPTS_ENERGY_SLASH: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipts_energy_slash_total",
        "Total number of energy slash receipts emitted across all blocks",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_ENERGY_SLASH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_energy_slash",
        "Aggregated ENERGY slash amounts (BLOCK) for the current block",
    )
});

/// Ad receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_AD_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_ad_per_block",
        "Number of ad receipts in current block",
    )
});

/// Total serialized bytes of receipts in current block
#[cfg(feature = "telemetry")]
pub static RECEIPT_BYTES_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipt_bytes_per_block",
        "Total serialized receipt bytes in current block",
    )
});

/// Settlement amounts by market type (gauges for current block)
#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_STORAGE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_storage",
        "Total storage receipt settlement (BLOCK) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_COMPUTE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_compute",
        "Total compute receipt settlement (BLOCK) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_COMPUTE_SLASH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_compute_slash",
        "Aggregated compute SLA slash amounts (BLOCK) for the current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_ENERGY: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_energy",
        "Total energy receipt settlement (BLOCK) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_AD: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_ad",
        "Total ad receipt settlement (BLOCK) in current block",
    )
});

/// Metrics derivation performance
#[cfg(feature = "telemetry")]
pub static METRICS_DERIVATION_DURATION_MS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "metrics_derivation_duration_ms",
        "Time to derive market metrics from receipts (milliseconds)",
    )
});

/// Receipt encoding failures (critical metric)
#[cfg(feature = "telemetry")]
pub static RECEIPT_ENCODING_FAILURES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_encoding_failures_total",
        "Total receipt encoding failures (CRITICAL - indicates data corruption risk)",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_DRAIN_DEPTH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_drain_depth",
        "Number of pending compute receipts drained for the current block",
    )
});

#[cfg(feature = "telemetry")]
pub static SLA_BREACH_DEPTH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "sla_breach_depth",
        "Outstanding compute SLA breach entries awaiting settlement",
    )
});

#[cfg(feature = "telemetry")]
pub static ORCHARD_ALLOC_FREE_DELTA: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "orchard_alloc_free_delta",
        "Difference between allocator alloc/free pairs observed in ORCHARD_TENSOR_PROFILE",
    )
});

#[cfg(feature = "telemetry")]
pub static PROOF_VERIFICATION_LATENCY_MS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "proof_verification_latency_ms",
        "Latency to verify a SNARK proof (milliseconds)",
    )
});

/// Receipt validation failures
#[cfg(feature = "telemetry")]
pub static RECEIPT_VALIDATION_FAILURES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_validation_failures_total",
        "Total receipt validation failures (malformed receipts)",
    )
});

/// Receipt decoding failures (when reading blocks)
#[cfg(feature = "telemetry")]
pub static RECEIPT_DECODING_FAILURES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_decoding_failures_total",
        "Total receipt decoding failures when reading blocks",
    )
});

/// Shard-level usage gauges.
#[cfg(feature = "telemetry")]
pub static RECEIPT_SHARD_USAGE_COUNT: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec(
        "receipt_shard_count_per_block",
        "Receipt count per shard in current block",
        &["shard"],
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SHARD_USAGE_BYTES: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec(
        "receipt_shard_bytes_per_block",
        "Serialized receipt bytes per shard in current block",
        &["shard"],
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SHARD_USAGE_VERIFY_UNITS: Lazy<IntGaugeVec> = Lazy::new(|| {
    register_int_gauge_vec(
        "receipt_shard_verify_units_per_block",
        "Deterministic verify units per shard in current block",
        &["shard"],
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_DA_SAMPLE_SUCCESS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_da_sample_success_total",
        "Successful receipt data-availability samples",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_DA_SAMPLE_FAILURE_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_da_sample_failure_total",
        "Failed receipt data-availability samples",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_AGG_SIG_MISMATCH_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_aggregate_sig_mismatch_total",
        "Aggregated receipt signature mismatches during validation",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_HEADER_MISMATCH_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_header_mismatch_total",
        "Per-shard receipt root mismatch versus header",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SHARD_DIVERSITY_VIOLATION_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_shard_diversity_violation_total",
        "Receipt shard placement diversity violations (provider/region/ASN)",
    )
});

/// Pending receipt depth gauges (per market)
#[cfg(feature = "telemetry")]
pub static PENDING_RECEIPTS_STORAGE: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "pending_receipts_storage",
        "Number of pending storage receipts waiting to be included in a block",
    )
});

#[cfg(feature = "telemetry")]
pub static PENDING_RECEIPTS_COMPUTE: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "pending_receipts_compute",
        "Number of pending compute receipts waiting to be included in a block",
    )
});

#[cfg(feature = "telemetry")]
pub static PENDING_RECEIPTS_ENERGY: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "pending_receipts_energy",
        "Number of pending energy receipts waiting to be included in a block",
    )
});

/// Receipt drain operations
#[cfg(feature = "telemetry")]
pub static RECEIPT_DRAIN_OPERATIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_drain_operations_total",
        "Total number of receipt drain operations across all markets",
    )
});

/// Record receipt emission and metrics
///
/// Call this after adding receipts to a block to update telemetry.
#[cfg(feature = "telemetry")]
pub fn record_receipts(receipts: &[Receipt], serialized_bytes: usize) {
    // Total count
    RECEIPTS_PER_BLOCK.set(receipts.len() as i64);
    RECEIPT_BYTES_PER_BLOCK.set(serialized_bytes as i64);

    // Count by type and accumulate settlements
    let mut storage_count = 0i64;
    let mut storage_settlement = 0.0;
    let mut compute_count = 0i64;
    let mut compute_settlement = 0.0;
    let mut compute_slash_count = 0i64;
    let mut compute_slash_settlement = 0.0;
    let mut energy_count = 0i64;
    let mut energy_settlement = 0.0;
    let mut energy_slash_count = 0i64;
    let mut energy_slash_settlement = 0.0;
    let mut ad_count = 0i64;
    let mut ad_settlement = 0.0;

    for receipt in receipts {
        let settlement = receipt.settlement_amount() as f64;
        match receipt {
            Receipt::Storage(_) => {
                storage_count += 1;
                storage_settlement += settlement;
                RECEIPTS_STORAGE.inc();
            }
            Receipt::Compute(_) => {
                compute_count += 1;
                compute_settlement += settlement;
                RECEIPTS_COMPUTE.inc();
            }
            Receipt::ComputeSlash(_) => {
                compute_count += 1;
                compute_settlement += settlement;
                compute_slash_count += 1;
                compute_slash_settlement += settlement;
                RECEIPTS_COMPUTE.inc();
                RECEIPTS_COMPUTE_SLASH.inc();
            }
            Receipt::Energy(_) => {
                energy_count += 1;
                energy_settlement += settlement;
                RECEIPTS_ENERGY.inc();
            }
            Receipt::EnergySlash(_) => {
                energy_count += 1;
                energy_settlement += settlement;
                energy_slash_count += 1;
                energy_slash_settlement += settlement;
                RECEIPTS_ENERGY.inc();
                RECEIPTS_ENERGY_SLASH.inc();
            }
            Receipt::Ad(_) => {
                ad_count += 1;
                ad_settlement += settlement;
                RECEIPTS_AD.inc();
            }
        }
    }

    // Update per-block gauges
    RECEIPTS_STORAGE_PER_BLOCK.set(storage_count);
    RECEIPTS_COMPUTE_PER_BLOCK.set(compute_count);
    RECEIPTS_ENERGY_PER_BLOCK.set(energy_count);
    RECEIPTS_ENERGY_SLASH_PER_BLOCK.set(energy_slash_count);
    RECEIPTS_AD_PER_BLOCK.set(ad_count);
    RECEIPTS_COMPUTE_SLASH_PER_BLOCK.set(compute_slash_count);

    // Update settlement amounts
    RECEIPT_SETTLEMENT_STORAGE.set(storage_settlement);
    RECEIPT_SETTLEMENT_COMPUTE.set(compute_settlement);
    RECEIPT_SETTLEMENT_ENERGY.set(energy_settlement);
    RECEIPT_SETTLEMENT_COMPUTE_SLASH.set(compute_slash_settlement);
    RECEIPT_SETTLEMENT_ENERGY_SLASH.set(energy_slash_settlement);
    RECEIPT_SETTLEMENT_AD.set(ad_settlement);
}

/// Stub for non-telemetry builds
#[cfg(not(feature = "telemetry"))]
pub fn record_receipts(_receipts: &[Receipt], _serialized_bytes: usize) {}

#[cfg(feature = "telemetry")]
pub fn record_shard_usage(shard: u16, usage: &ReceiptBlockUsage) {
    let label = shard.to_string();
    RECEIPT_SHARD_USAGE_COUNT
        .with_label_values(&[&label])
        .set(usage.count as i64);
    RECEIPT_SHARD_USAGE_BYTES
        .with_label_values(&[&label])
        .set(usage.bytes as i64);
    RECEIPT_SHARD_USAGE_VERIFY_UNITS
        .with_label_values(&[&label])
        .set(usage.verify_units as i64);
}

#[cfg(not(feature = "telemetry"))]
pub fn record_shard_usage(_shard: u16, _usage: &ReceiptBlockUsage) {}

#[cfg(feature = "telemetry")]
pub fn set_receipt_drain_depth(value: usize) {
    RECEIPT_DRAIN_DEPTH.set(value as f64);
}

#[cfg(feature = "telemetry")]
pub fn set_sla_breach_depth(value: usize) {
    SLA_BREACH_DEPTH.set(value as f64);
}

#[cfg(feature = "telemetry")]
pub fn set_orchard_alloc_free_delta(delta: i64) {
    ORCHARD_ALLOC_FREE_DELTA.set(delta as f64);
}

#[cfg(feature = "telemetry")]
pub fn record_proof_verification_latency(duration: Duration) {
    PROOF_VERIFICATION_LATENCY_MS.observe(duration.as_secs_f64() * 1000.0);
    blocktorch_update_metadata(|meta| {
        meta.proof_latency_ms = Some(duration.as_secs_f64() * 1000.0)
    });
}

#[cfg(feature = "telemetry")]
pub fn set_blocktorch_kernel_digest(digest: [u8; 32]) {
    blocktorch_update_metadata(|meta| meta.kernel_digest = Some(hex::encode(digest)));
}

#[cfg(feature = "telemetry")]
pub fn set_blocktorch_benchmark_commit(commit: Option<&str>) {
    blocktorch_update_metadata(|meta| {
        meta.benchmark_commit = commit.map(|value| value.to_string());
    });
}

#[cfg(feature = "telemetry")]
pub fn set_blocktorch_tensor_profile_epoch(epoch: Option<&str>) {
    blocktorch_update_metadata(|meta| {
        meta.tensor_profile_epoch = epoch.map(|value| value.to_string());
    });
}

/// Record metrics derivation time
#[cfg(feature = "telemetry")]
pub fn record_metrics_derivation_time_ms(duration_ms: u64) {
    METRICS_DERIVATION_DURATION_MS.observe(duration_ms as f64);
}

/// Stub for non-telemetry builds
#[cfg(not(feature = "telemetry"))]
pub fn record_metrics_derivation_time_ms(_duration_ms: u64) {}
