//! Integration tests for circuit breaker in treasury executor.
//!
//! These tests validate that the circuit breaker correctly prevents cascading failures
//! during repeated submission errors while allowing quick recovery when the service heals.

#[cfg(test)]
mod tests {
    use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
    use crate::store::{TreasuryExecutorConfig, TreasuryExecutorError};
    use crate::treasury::{SignedExecutionIntent, TreasuryDisbursement, DisbursementStatus};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;

    /// Test that circuit breaker opens after threshold failures
    #[test]
    fn test_circuit_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout_secs: 1,
            window_secs: 300,
        };
        let breaker = CircuitBreaker::new(config);

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());

        // Record failures up to threshold
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert_eq!(breaker.failure_count(), 1);

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert_eq!(breaker.failure_count(), 2);

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert_eq!(breaker.failure_count(), 3);
        assert!(!breaker.allow_request());
    }

    /// Test that circuit transitions to half-open after timeout
    #[test]
    fn test_circuit_transitions_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_secs: 1, // Short timeout for testing
            window_secs: 300,
        };
        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request());

        // Wait for timeout
        std::thread::sleep(Duration::from_secs(2));

        // Should transition to half-open on next allow_request call
        assert!(breaker.allow_request());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
    }

    /// Test that circuit closes after successes in half-open state
    #[test]
    fn test_circuit_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_secs: 0, // Immediate transition for testing
            window_secs: 300,
        };
        let breaker = CircuitBreaker::new(config);

        // Force to open
        breaker.force_open();
        assert_eq!(breaker.state(), CircuitState::Open);

        // Manually transition to half-open (simulating timeout)
        breaker.force_close();
        breaker.force_open();
        // After timeout, it should go half-open
        std::thread::sleep(Duration::from_millis(10));
        breaker.allow_request();

        // Record enough successes to close
        breaker.record_success();
        assert_eq!(breaker.success_count(), 1);

        breaker.record_success();
        // Should transition to closed after success_threshold successes
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert_eq!(breaker.failure_count(), 0);
    }

    /// Test that circuit reopens on failure in half-open state
    #[test]
    fn test_circuit_reopens_on_half_open_failure() {
        let breaker = CircuitBreaker::default();

        // Force to half-open
        breaker.force_open();
        std::thread::sleep(Duration::from_millis(10));
        breaker.state.store(CircuitState::HalfOpen as u8, Ordering::Release);
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Any failure in half-open should reopen
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    /// Test error classification: only transient submission errors count
    #[test]
    fn test_error_classification() {
        let breaker = Arc::new(CircuitBreaker::default());

        // Storage errors should NOT count against circuit
        let storage_err = TreasuryExecutorError::Storage("db corrupted".into());
        assert!(storage_err.is_storage());
        // Breaker should not be called for storage errors (they return early)

        // Cancelled errors should NOT count
        let cancelled_err = TreasuryExecutorError::cancelled("insufficient balance");
        assert!(cancelled_err.is_cancelled());
        // Breaker should not be called for cancelled errors

        // Submission errors SHOULD count
        let submission_err = TreasuryExecutorError::Submission("RPC timeout".into());
        assert!(!submission_err.is_storage());
        assert!(!submission_err.is_cancelled());

        // Simulate the executor logic
        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    /// Test concurrent access to circuit breaker
    #[test]
    fn test_concurrent_circuit_breaker() {
        let breaker = Arc::new(CircuitBreaker::default());
        let mut handles = vec![];

        // Spawn 10 threads recording failures
        for _ in 0..10 {
            let breaker_clone = Arc::clone(&breaker);
            handles.push(std::thread::spawn(move || {
                breaker_clone.record_failure();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have recorded all failures
        assert!(breaker.failure_count() >= 5);
        // Circuit should be open
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    /// Test circuit breaker state persistence across requests
    #[test]
    fn test_state_persistence() {
        let breaker = CircuitBreaker::default();

        // Open the circuit
        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        // State should persist across multiple allow_request calls within timeout
        for _ in 0..10 {
            assert!(!breaker.allow_request());
            assert_eq!(breaker.state(), CircuitState::Open);
        }
    }

    /// Test force operations for manual intervention
    #[test]
    fn test_manual_intervention() {
        let breaker = CircuitBreaker::default();

        // Force open
        breaker.force_open();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request());

        // Force close (manual override for emergencies)
        breaker.force_close();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());

        // Reset clears all state
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);

        breaker.reset();
        assert_eq!(breaker.failure_count(), 0);
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    /// Test production-ready configuration values
    #[test]
    fn test_production_config() {
        let config = CircuitBreakerConfig {
            failure_threshold: 5,      // Open after 5 failures
            success_threshold: 2,       // Close after 2 successes
            timeout_secs: 60,          // Stay open for 60 seconds
            window_secs: 300,          // 5 minute failure window
        };
        let breaker = CircuitBreaker::new(config);

        // Should handle typical production failure scenario
        for _ in 0..4 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Closed);

        // 5th failure opens circuit
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request());
    }
}
