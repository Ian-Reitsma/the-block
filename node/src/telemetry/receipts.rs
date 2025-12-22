//! Receipt telemetry and metrics tracking
//!
//! Tracks receipt emission, serialization size, and derivation performance.
//! Enables monitoring of market activity at the consensus level.

#[cfg(feature = "telemetry")]
use super::{register_counter, register_gauge, register_histogram, register_int_gauge};
use crate::receipts::Receipt;
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::{Gauge, Histogram, IntCounter, IntGauge};

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

/// Energy receipt count per block (gauge)
#[cfg(feature = "telemetry")]
pub static RECEIPTS_ENERGY_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "receipts_energy_per_block",
        "Number of energy receipts in current block",
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
        "receipt_settlement_storage_ct",
        "Total storage receipt settlement (CT) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_COMPUTE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_compute_ct",
        "Total compute receipt settlement (CT) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_ENERGY: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_energy_ct",
        "Total energy receipt settlement (CT) in current block",
    )
});

#[cfg(feature = "telemetry")]
pub static RECEIPT_SETTLEMENT_AD: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "receipt_settlement_ad_ct",
        "Total ad receipt settlement (CT) in current block",
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
    let mut energy_count = 0i64;
    let mut energy_settlement = 0.0;
    let mut ad_count = 0i64;
    let mut ad_settlement = 0.0;

    for receipt in receipts {
        let settlement_ct = receipt.settlement_amount() as f64;
        match receipt {
            Receipt::Storage(_) => {
                storage_count += 1;
                storage_settlement += settlement_ct;
                RECEIPTS_STORAGE.inc();
            }
            Receipt::Compute(_) => {
                compute_count += 1;
                compute_settlement += settlement_ct;
                RECEIPTS_COMPUTE.inc();
            }
            Receipt::Energy(_) => {
                energy_count += 1;
                energy_settlement += settlement_ct;
                RECEIPTS_ENERGY.inc();
            }
            Receipt::Ad(_) => {
                ad_count += 1;
                ad_settlement += settlement_ct;
                RECEIPTS_AD.inc();
            }
        }
    }

    // Update per-block gauges
    RECEIPTS_STORAGE_PER_BLOCK.set(storage_count);
    RECEIPTS_COMPUTE_PER_BLOCK.set(compute_count);
    RECEIPTS_ENERGY_PER_BLOCK.set(energy_count);
    RECEIPTS_AD_PER_BLOCK.set(ad_count);

    // Update settlement amounts
    RECEIPT_SETTLEMENT_STORAGE.set(storage_settlement);
    RECEIPT_SETTLEMENT_COMPUTE.set(compute_settlement);
    RECEIPT_SETTLEMENT_ENERGY.set(energy_settlement);
    RECEIPT_SETTLEMENT_AD.set(ad_settlement);
}

/// Stub for non-telemetry builds
#[cfg(not(feature = "telemetry"))]
pub fn record_receipts(_receipts: &[Receipt], _serialized_bytes: usize) {}

/// Record metrics derivation time
#[cfg(feature = "telemetry")]
pub fn record_metrics_derivation_time_ms(duration_ms: u64) {
    METRICS_DERIVATION_DURATION_MS.observe(duration_ms as f64);
}

/// Stub for non-telemetry builds
#[cfg(not(feature = "telemetry"))]
pub fn record_metrics_derivation_time_ms(_duration_ms: u64) {}
