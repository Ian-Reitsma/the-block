//! Treasury-specific telemetry and metrics.
//!
//! This module defines the canonical metrics for treasury operations, executor health,
//! and disbursement lifecycle tracking. All metrics follow the naming conventions from
//! AGENTS.md:1112-1113 and feed into the Grafana dashboards under monitoring/.
//!
//! # Security
//! - All label values are validated and sanitized to prevent metric cardinality explosion
//! - User-provided labels are mapped to predefined constants where possible
//! - Unknown labels are mapped to "other" to bound cardinality

#[cfg(feature = "telemetry")]
use super::{register_counter_vec, register_gauge, register_gauge_vec, register_histogram};
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::{Gauge, GaugeVec, Histogram, IntCounterVec};

// ========================================
// LABEL SANITIZATION & CARDINALITY LIMITS
// ========================================

/// Validate and sanitize a status label to prevent cardinality explosion
fn sanitize_status_label(status: &str) -> &'static str {
    match status {
        "draft" => status::DRAFT,
        "voting" => status::VOTING,
        "queued" => status::QUEUED,
        "timelocked" => status::TIMELOCKED,
        "executed" => status::EXECUTED,
        "finalized" => status::FINALIZED,
        "rolled_back" => status::ROLLED_BACK,
        _ => "other",
    }
}

/// Validate and sanitize an error reason label
fn sanitize_error_reason_label(reason: &str) -> &'static str {
    match reason {
        r if r.contains("insufficient") || r.contains("balance") => {
            error_reason::INSUFFICIENT_FUNDS
        }
        r if r.contains("target") || r.contains("destination") => error_reason::INVALID_TARGET,
        r if r.contains("stale") || r.contains("dependency") => error_reason::STALE_DEPENDENCY,
        r if r.contains("circular") => error_reason::CIRCULAR_DEPENDENCY,
        r if r.contains("execution") || r.contains("failed") => error_reason::EXECUTION_FAILED,
        r if r.contains("quorum") => error_reason::QUORUM_NOT_REACHED,
        r if r.contains("epoch") || r.contains("expired") => error_reason::EPOCH_EXPIRED,
        _ => "other",
    }
}

/// Validate and sanitize a dependency failure type label
fn sanitize_dependency_failure_label(failure_type: &str) -> &'static str {
    match failure_type {
        "circular" => dependency_failure::CIRCULAR,
        "missing" => dependency_failure::MISSING,
        "stale" => dependency_failure::STALE,
        "rolled_back_dependency" => dependency_failure::ROLLED_BACK,
        _ => "other",
    }
}

// ========================================
// DISBURSEMENT LIFECYCLE COUNTERS
// ========================================

/// Total disbursements by status (draft, voting, queued, timelocked, executed, finalized, rolled_back)
#[cfg(feature = "telemetry")]
static GOVERNANCE_DISBURSEMENTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "governance_disbursements_total",
        "Total number of treasury disbursements by final status",
        &["status"],
    )
});

/// Treasury execution errors by reason
#[cfg(feature = "telemetry")]
static TREASURY_EXECUTION_ERRORS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "treasury_execution_errors_total",
        "Total execution errors categorized by failure reason",
        &["reason"],
    )
});

// ========================================
// TREASURY BALANCE GAUGES
// ========================================

/// Current treasury balance in CT
#[cfg(feature = "telemetry")]
static TREASURY_BALANCE_CT: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "treasury_balance_ct",
        "Current treasury balance in CT (consumer tokens)",
    )
});

/// Current treasury balance in IT
#[cfg(feature = "telemetry")]
static TREASURY_BALANCE_IT: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "treasury_balance_it",
        "Current treasury balance in IT (industrial tokens)",
    )
});

// ========================================
// DISBURSEMENT BACKLOG GAUGES
// ========================================

/// Number of pending disbursements by status
#[cfg(feature = "telemetry")]
static TREASURY_DISBURSEMENT_BACKLOG: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec(
        "treasury_disbursement_backlog",
        "Current count of pending disbursements awaiting execution, by status",
        &["status"],
    )
});

// ========================================
// CIRCUIT BREAKER GAUGE
// ========================================

/// Current state of treasury executor circuit breaker
/// Values: 0 = Closed (normal), 1 = Open (rejecting), 2 = Half-Open (testing recovery)
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_STATE: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "treasury_circuit_breaker_state",
        "Current state of treasury executor circuit breaker: 0=closed, 1=open, 2=half_open",
    )
});

/// Failure count in current circuit breaker window
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_FAILURES: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "treasury_circuit_breaker_failures",
        "Current failure count in circuit breaker window",
    )
});

/// Success count in half-open state
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_SUCCESSES: Lazy<Gauge> = Lazy::new(|| {
    register_gauge(
        "treasury_circuit_breaker_successes",
        "Consecutive successes in half-open state",
    )
});

// ========================================
// EXECUTION LATENCY HISTOGRAM
// ========================================

/// Time from queued to executed (seconds)
#[cfg(feature = "telemetry")]
static TREASURY_DISBURSEMENT_LAG_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "treasury_disbursement_lag_seconds",
        "Duration from disbursement queued to executed (seconds)",
    )
});

/// Executor tick duration (seconds)
#[cfg(feature = "telemetry")]
static TREASURY_EXECUTOR_TICK_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "treasury_executor_tick_duration_seconds",
        "Time taken by treasury executor to process one tick",
    )
});

// ========================================
// DEPENDENCY TRACKING COUNTERS
// ========================================

/// Dependency validation failures
#[cfg(feature = "telemetry")]
static TREASURY_DEPENDENCY_FAILURES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    register_counter_vec(
        "treasury_dependency_failures_total",
        "Total dependency validation failures (circular, stale, missing)",
        &["failure_type"],
    )
});

// ========================================
// PUBLIC API
// ========================================

/// Increment disbursement counter for a specific status
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion
#[cfg(feature = "telemetry")]
pub fn increment_disbursements(status: &str) {
    let sanitized = sanitize_status_label(status);
    GOVERNANCE_DISBURSEMENTS_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_disbursements(_status: &str) {}

/// Increment execution error counter for a specific reason
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion from user-provided error messages
#[cfg(feature = "telemetry")]
pub fn increment_execution_error(reason: &str) {
    let sanitized = sanitize_error_reason_label(reason);
    TREASURY_EXECUTION_ERRORS_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_execution_error(_reason: &str) {}

/// Increment dependency failure counter
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion
#[cfg(feature = "telemetry")]
pub fn increment_dependency_failure(failure_type: &str) {
    let sanitized = sanitize_dependency_failure_label(failure_type);
    TREASURY_DEPENDENCY_FAILURES_TOTAL
        .with_label_values(&[sanitized])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_dependency_failure(_failure_type: &str) {}

/// Update treasury balance gauges
#[cfg(feature = "telemetry")]
pub fn set_treasury_balance(ct: u64, it: u64) {
    TREASURY_BALANCE_CT.set(ct as f64);
    TREASURY_BALANCE_IT.set(it as f64);
}

#[cfg(not(feature = "telemetry"))]
pub fn set_treasury_balance(_ct: u64, _it: u64) {}

/// Update disbursement backlog gauge for a specific status
///
/// # Security
/// Labels are sanitized to prevent cardinality explosion
#[cfg(feature = "telemetry")]
pub fn set_disbursement_backlog(status: &str, count: usize) {
    let sanitized = sanitize_status_label(status);
    TREASURY_DISBURSEMENT_BACKLOG
        .with_label_values(&[sanitized])
        .set(count as f64);
}

#[cfg(not(feature = "telemetry"))]
pub fn set_disbursement_backlog(_status: &str, _count: usize) {}

/// Record disbursement lag (queued_at -> executed_at)
#[cfg(feature = "telemetry")]
pub fn observe_disbursement_lag(seconds: f64) {
    TREASURY_DISBURSEMENT_LAG_SECONDS.observe(seconds);
}

#[cfg(not(feature = "telemetry"))]
pub fn observe_disbursement_lag(_seconds: f64) {}

/// Record executor tick duration
#[cfg(feature = "telemetry")]
pub fn observe_executor_tick_duration(seconds: f64) {
    TREASURY_EXECUTOR_TICK_DURATION_SECONDS.observe(seconds);
}

#[cfg(not(feature = "telemetry"))]
pub fn observe_executor_tick_duration(_seconds: f64) {}

/// Update circuit breaker state gauge
///
/// This should be called whenever the circuit breaker state changes or periodically
/// from the executor loop to ensure Prometheus has current state.
///
/// # Arguments
/// * `state` - Current circuit breaker state (0=closed, 1=open, 2=half_open)
/// * `failures` - Current failure count in the window
/// * `successes` - Current success count (relevant in half-open state)
#[cfg(feature = "telemetry")]
pub fn set_circuit_breaker_state(state: u8, failures: u64, successes: u64) {
    TREASURY_CIRCUIT_BREAKER_STATE.set(state as f64);
    TREASURY_CIRCUIT_BREAKER_FAILURES.set(failures as f64);
    TREASURY_CIRCUIT_BREAKER_SUCCESSES.set(successes as f64);
}

#[cfg(not(feature = "telemetry"))]
pub fn set_circuit_breaker_state(_state: u8, _failures: u64, _successes: u64) {}

// ========================================
// STATUS CONSTANTS
// ========================================

/// Status label constants for consistency
pub mod status {
    pub const DRAFT: &str = "draft";
    pub const VOTING: &str = "voting";
    pub const QUEUED: &str = "queued";
    pub const TIMELOCKED: &str = "timelocked";
    pub const EXECUTED: &str = "executed";
    pub const FINALIZED: &str = "finalized";
    pub const ROLLED_BACK: &str = "rolled_back";
}

/// Error reason constants
pub mod error_reason {
    pub const INSUFFICIENT_FUNDS: &str = "insufficient_funds";
    pub const INVALID_TARGET: &str = "invalid_target";
    pub const STALE_DEPENDENCY: &str = "stale_dependency";
    pub const CIRCULAR_DEPENDENCY: &str = "circular_dependency";
    pub const EXECUTION_FAILED: &str = "execution_failed";
    pub const QUORUM_NOT_REACHED: &str = "quorum_not_reached";
    pub const EPOCH_EXPIRED: &str = "epoch_expired";
}

/// Dependency failure type constants
pub mod dependency_failure {
    pub const CIRCULAR: &str = "circular";
    pub const MISSING: &str = "missing";
    pub const STALE: &str = "stale";
    pub const ROLLED_BACK: &str = "rolled_back_dependency";
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;

    #[test]
    fn test_increment_disbursements() {
        increment_disbursements(status::DRAFT);
        increment_disbursements(status::FINALIZED);
    }

    #[test]
    fn test_set_treasury_balance() {
        set_treasury_balance(1_000_000, 500_000);
    }

    #[test]
    fn test_observe_lag() {
        observe_disbursement_lag(42.5);
        observe_executor_tick_duration(0.125);
    }

    #[test]
    fn test_dependency_failures() {
        increment_dependency_failure(dependency_failure::CIRCULAR);
        increment_dependency_failure(dependency_failure::MISSING);
    }
}
