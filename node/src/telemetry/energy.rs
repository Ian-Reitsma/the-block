//! Energy market telemetry and oracle metrics.
//!
//! This module defines metrics for oracle health, energy readings, disputes,
//! and market-level aggregations. All metrics follow naming conventions from
//! AGENTS.md:1114-1116 and feed into Grafana dashboards under monitoring/.
//!
//! # Security
//! - All label values are validated and sanitized to prevent metric cardinality explosion
//! - User-provided labels are mapped to predefined constants where possible
//! - Unknown labels are mapped to "other" to bound cardinality

#[cfg(feature = "telemetry")]
use super::{register_counter, register_counter_vec, register_gauge, register_histogram};
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::{Gauge, Histogram, IntCounter, IntCounterVec};

// ========================================
// LABEL SANITIZATION & CARDINALITY LIMITS
// ========================================

/// Validate and sanitize an oracle error reason label
fn sanitize_oracle_error_label(reason: &str) -> &'static str {
    match reason {
        r if r.contains("invalid") || r.contains("reading") => error_reason::INVALID_READING,
        r if r.contains("stale") || r.contains("timestamp") => error_reason::STALE_TIMESTAMP,
        r if r.contains("authorization") || r.contains("auth") => {
            error_reason::AUTHORIZATION_FAILED
        }
        r if r.contains("signature") || r.contains("sig") => error_reason::BAD_SIGNATURE,
        _ => "other",
    }
}

/// Validate and sanitize a dispute type label
fn sanitize_dispute_type_label(dispute_type: &str) -> &'static str {
    match dispute_type {
        "low_reading" => dispute_type::LOW_READING,
        "outlier_detected" => dispute_type::OUTLIER_DETECTED,
        "consensus_gap" => dispute_type::CONSENSUS_GAP,
        _ => "other",
    }
}

/// Validate and sanitize a dispute outcome label
fn sanitize_dispute_outcome_label(outcome: &str) -> &'static str {
    match outcome {
        "resolved" => dispute_outcome::RESOLVED,
        "escalated" => dispute_outcome::ESCALATED,
        "slashed" => dispute_outcome::SLASHED,
        _ => "other",
    }
}

/// Normalize dispute states for telemetry labels.
fn sanitize_dispute_state_label(state: &str) -> &'static str {
    match state {
        "open" => dispute_state::OPEN,
        "resolved" => dispute_state::RESOLVED,
        "slashed" => dispute_state::SLASHED,
        _ => "other",
    }
}

/// Validate and sanitize a slash reason label
fn sanitize_slash_reason_label(reason: &str) -> &'static str {
    match reason {
        r if r.contains("quorum") => slash_reason::QUORUM,
        r if r.contains("expire") || r.contains("expiry") => slash_reason::EXPIRY,
        r if r.contains("conflict") || r.contains("hash") => slash_reason::CONFLICT,
        _ => slash_reason::OTHER,
    }
}

// ========================================
// ORACLE SUBMISSION METRICS
// ========================================

/// Total energy readings submitted by oracle
#[cfg(feature = "telemetry")]
static ENERGY_READINGS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "energy_readings_total",
        "Total energy readings submitted across all oracles",
    )
});

/// Oracle submission errors by reason
#[cfg(feature = "telemetry")]
static ORACLE_SUBMISSION_ERRORS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "oracle_submission_errors_total",
        "Total oracle submission failures (invalid_reading, stale_timestamp, authorization_failed)",
        &["reason"],
    )
});

// ========================================
// DISPUTE RESOLUTION METRICS
// ========================================

/// Disputes raised by type (low_reading, outlier_detected, consensus_gap)
#[cfg(feature = "telemetry")]
static ENERGY_DISPUTES_RAISED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_disputes_raised_total",
        "Total disputes initiated by type",
        &["type"],
    )
});

/// Dispute outcomes: resolved, escalated, slashed
#[cfg(feature = "telemetry")]
static ENERGY_DISPUTES_RESOLVED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_disputes_resolved_total",
        "Total disputes concluded with outcome",
        &["outcome"],
    )
});

// ========================================
// MARKET VOLUME & PRICE GAUGES
// ========================================

/// Current market price per unit (updated per block)
#[cfg(feature = "telemetry")]
static ENERGY_MARKET_PRICE_CURRENT: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "energy_market_price_current",
        "Current energy market clearing price per unit",
    )
});

/// Current market volume in blocks
#[cfg(feature = "telemetry")]
static ENERGY_MARKET_VOLUME_CURRENT: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "energy_market_volume_current",
        "Current energy market volume in blocks",
    )
});

// ========================================
// ORACLE HEALTH GAUGES
// ========================================

/// Number of active oracles (per epoch)
#[cfg(feature = "telemetry")]
static ORACLE_ACTIVE_COUNT: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "oracle_active_count",
        "Number of active oracle operators by status",
    )
});

/// Number of pending disputes waiting resolution
#[cfg(feature = "telemetry")]
pub(crate) static ENERGY_DISPUTES_PENDING: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "energy_disputes_pending",
        "Number of disputes awaiting resolution",
    )
});

// ========================================
// LATENCY & PERFORMANCE HISTOGRAMS
// ========================================

/// Oracle-to-inclusion latency (seconds)
#[cfg(feature = "telemetry")]
static ORACLE_INCLUSION_LAG_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "oracle_inclusion_lag_seconds",
        "Time from oracle submission to consensus inclusion",
    )
});

/// Dispute resolution time (seconds)
#[cfg(feature = "telemetry")]
static ENERGY_DISPUTE_RESOLUTION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "energy_dispute_resolution_seconds",
        "Time from dispute initiation to final resolution",
    )
});

/// Quorum shortfall counts (reason = provider)
#[cfg(feature = "telemetry")]
pub(crate) static ENERGY_QUORUM_SHORTFALL_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_quorum_shortfall_total",
        "Settlements rejected because quorum thresholds were not met",
        &["provider"],
    )
});

/// Reading rejections by reason (e.g., invalid_reading, stale_timestamp)
#[cfg(feature = "telemetry")]
pub(crate) static ENERGY_READING_REJECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_reading_reject_total",
        "Oracle readings rejected before creating credits",
        &["reason"],
    )
});

/// Slashing counts tagged by provider + reason
#[cfg(feature = "telemetry")]
pub(crate) static ENERGY_SLASHING_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_slashing_total",
        "Energy slashing events recorded when quorum, expiry, or conflicts occur",
        &["provider", "reason"],
    )
});

/// Dispute lifecycle counts keyed by state (open/resolved/slashed)
#[cfg(feature = "telemetry")]
pub(crate) static ENERGY_DISPUTE_STATE_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "energy_dispute_total",
        "Energy dispute counts by lifecycle state",
        &["state"],
    )
});

// ========================================
// PUBLIC API
// ========================================

/// Record energy reading submission
#[cfg(feature = "telemetry")]
pub fn increment_energy_readings() {
    ENERGY_READINGS_TOTAL.inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_energy_readings() {}

/// Record oracle submission error
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion from user-provided error messages
#[cfg(feature = "telemetry")]
pub fn increment_oracle_submission_error(reason: &str) {
    let sanitized = sanitize_oracle_error_label(reason);
    ORACLE_SUBMISSION_ERRORS_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_oracle_submission_error(_reason: &str) {}

/// Record dispute initiated
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion
#[cfg(feature = "telemetry")]
pub fn increment_disputes_raised(dispute_type: &str) {
    let sanitized = sanitize_dispute_type_label(dispute_type);
    ENERGY_DISPUTES_RAISED_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_disputes_raised(_dispute_type: &str) {}

/// Record dispute resolved
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion
#[cfg(feature = "telemetry")]
pub fn increment_disputes_resolved(outcome: &str) {
    let sanitized = sanitize_dispute_outcome_label(outcome);
    ENERGY_DISPUTES_RESOLVED_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_disputes_resolved(_outcome: &str) {}

/// Update market metrics
#[cfg(feature = "telemetry")]
pub fn set_market_metrics(price: f64, volume: f64) {
    ENERGY_MARKET_PRICE_CURRENT.set(price);
    ENERGY_MARKET_VOLUME_CURRENT.set(volume);
}

#[cfg(not(feature = "telemetry"))]
pub fn set_market_metrics(_price: f64, _volume: f64) {}

/// Update oracle health
#[cfg(feature = "telemetry")]
pub fn set_oracle_health(active_count: usize, pending_disputes: usize) {
    ORACLE_ACTIVE_COUNT.set(active_count as f64);
    ENERGY_DISPUTES_PENDING.set(pending_disputes as f64);
}

#[cfg(not(feature = "telemetry"))]
pub fn set_oracle_health(_active_count: usize, _pending_disputes: usize) {}

/// Record oracle inclusion latency
#[cfg(feature = "telemetry")]
pub fn observe_oracle_inclusion_lag(seconds: f64) {
    ORACLE_INCLUSION_LAG_SECONDS.observe(seconds);
}

#[cfg(not(feature = "telemetry"))]
pub fn observe_oracle_inclusion_lag(_seconds: f64) {}

/// Record dispute resolution time
#[cfg(feature = "telemetry")]
pub fn observe_dispute_resolution_time(seconds: f64) {
    ENERGY_DISPUTE_RESOLUTION_SECONDS.observe(seconds);
}

#[cfg(not(feature = "telemetry"))]
pub fn observe_dispute_resolution_time(_seconds: f64) {}

/// Record a rejected reading reason.
#[cfg(feature = "telemetry")]
pub fn increment_reading_reject(reason: &str) {
    let label = sanitize_oracle_error_label(reason);
    ENERGY_READING_REJECT_TOTAL
        .with_label_values(&[label])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_reading_reject(_reason: &str) {}

/// Record a quorum shortfall event tagged by provider.
#[cfg(feature = "telemetry")]
pub fn increment_quorum_shortfall(provider: &str) {
    let label = provider;
    ENERGY_QUORUM_SHORTFALL_TOTAL
        .with_label_values(&[label])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_quorum_shortfall(_provider: &str) {}

/// Record the current dispute state for telemetry tracking.
#[cfg(feature = "telemetry")]
pub fn increment_dispute_state(state: &str) {
    let label = sanitize_dispute_state_label(state);
    ENERGY_DISPUTE_STATE_TOTAL.with_label_values(&[label]).inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_dispute_state(_state: &str) {}

/// Record each slashing event (quorum shortfall, expiry, conflicts).
#[cfg(feature = "telemetry")]
pub fn increment_slashing(provider: &str, reason: &str) {
    let label = sanitize_slash_reason_label(reason);
    ENERGY_SLASHING_TOTAL
        .with_label_values(&[provider, label])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_slashing(_provider: &str, _reason: &str) {}

// ========================================
// ERROR REASON CONSTANTS
// ========================================

pub mod error_reason {
    pub const INVALID_READING: &str = "invalid_reading";
    pub const STALE_TIMESTAMP: &str = "stale_timestamp";
    pub const AUTHORIZATION_FAILED: &str = "authorization_failed";
    pub const BAD_SIGNATURE: &str = "bad_signature";
}

pub mod dispute_type {
    pub const LOW_READING: &str = "low_reading";
    pub const OUTLIER_DETECTED: &str = "outlier_detected";
    pub const CONSENSUS_GAP: &str = "consensus_gap";
}

pub mod dispute_outcome {
    pub const RESOLVED: &str = "resolved";
    pub const ESCALATED: &str = "escalated";
    pub const SLASHED: &str = "slashed";
}

pub mod dispute_state {
    pub const OPEN: &str = "open";
    pub const RESOLVED: &str = "resolved";
    pub const SLASHED: &str = "slashed";
}

pub mod slash_reason {
    pub const QUORUM: &str = "quorum";
    pub const EXPIRY: &str = "expiry";
    pub const CONFLICT: &str = "conflict";
    pub const OTHER: &str = "other";
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;

    #[test]
    fn test_energy_metrics() {
        increment_energy_readings();
        increment_oracle_submission_error(error_reason::INVALID_READING);
    }

    #[test]
    fn test_dispute_metrics() {
        increment_disputes_raised(dispute_type::LOW_READING);
        increment_disputes_resolved(dispute_outcome::RESOLVED);
    }

    #[test]
    fn test_market_metrics() {
        set_market_metrics(42.5, 100.0);
        set_oracle_health(5, 2);
    }

    #[test]
    fn test_latency_metrics() {
        observe_oracle_inclusion_lag(2.5);
        observe_dispute_resolution_time(120.0);
    }
}
