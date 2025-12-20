//! Circuit Breaker for Treasury Executor
//!
//! Implements the circuit breaker pattern to prevent cascading failures
//! when the treasury executor encounters repeated errors.
//!
//! States:
//! - **Closed**: Normal operation, requests flow through
//! - **Open**: Too many failures, reject all requests immediately
//! - **Half-Open**: Testing if service recovered, allow limited requests
//!
//! Configuration:
//! - Failure threshold: Number of failures before opening circuit
//! - Timeout: How long circuit stays open before attempting recovery
//! - Success threshold: Consecutive successes needed to close from half-open

use diagnostics::{info, warn};
use foundation_serialization::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum CircuitState {
    /// Normal operation - all requests allowed
    Closed = 0,
    /// Too many failures - reject requests immediately
    Open = 1,
    /// Testing recovery - allow limited requests
    HalfOpen = 2,
}

impl From<u8> for CircuitState {
    fn from(value: u8) -> Self {
        match value {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u64,
    /// Number of consecutive successes needed to close from half-open
    pub success_threshold: u64,
    /// How long circuit stays open before attempting recovery (seconds)
    pub timeout_secs: u64,
    /// Window size for tracking failures (seconds)
    pub window_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5, // Open after 5 failures
            success_threshold: 2, // Close after 2 successes
            timeout_secs: 60,     // Stay open for 60 seconds
            window_secs: 300,     // 5 minute failure window
        }
    }
}

/// Circuit breaker for treasury operations
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: AtomicU8,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: AtomicU64,
    last_state_change: AtomicU64,
    start_time: Instant,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        let start_time = Instant::now();
        Self {
            config,
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: AtomicU64::new(u64::MAX),
            last_state_change: AtomicU64::new(0),
            start_time,
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        CircuitState::from(self.state.load(Ordering::Acquire))
    }

    /// Check if operation is allowed
    ///
    /// Returns true if the operation should proceed, false if it should be rejected
    pub fn allow_request(&self) -> bool {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let elapsed = self.elapsed_since_state_change();

                if elapsed.as_secs() >= self.config.timeout_secs {
                    self.transition_to_half_open();
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                true
            }
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Release);
            }
            CircuitState::HalfOpen => {
                // Increment success count
                let successes = self.success_count.fetch_add(1, Ordering::AcqRel) + 1;

                // Close circuit if enough successes
                if successes >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Ignore successes in open state (shouldn't happen)
            }
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        self.last_failure_time
            .store(self.offset_now(), Ordering::Release);

        let current_state = self.state();

        match current_state {
            CircuitState::Closed => {
                // Check if failure is within window
                let failures = self.failure_count.fetch_add(1, Ordering::AcqRel) + 1;

                // Open circuit if threshold reached
                if failures >= self.config.failure_threshold {
                    self.transition_to_open();
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state reopens circuit
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Already open, just increment counter
                self.failure_count.fetch_add(1, Ordering::Release);
            }
        }
    }

    /// Force circuit to open state (REQUIRES AUTHORIZATION - use with authorized_force_open)
    ///
    /// # Security
    /// This method should only be called after verifying authorization via
    /// `crate::authorization::verify_authorization` with `Role::Operator`.
    /// Direct calls bypass security and should be limited to tests.
    #[cfg(test)]
    pub fn force_open(&self) {
        self.transition_to_open();
    }

    /// Force circuit to closed state (REQUIRES AUTHORIZATION - use with authorized_force_close)
    ///
    /// # Security
    /// This method should only be called after verifying authorization via
    /// `crate::authorization::verify_authorization` with `Role::Operator`.
    /// Direct calls bypass security and should be limited to tests.
    #[cfg(test)]
    pub fn force_close(&self) {
        self.transition_to_closed();
    }

    /// Reset all counters (REQUIRES AUTHORIZATION - use with authorized_reset)
    ///
    /// # Security
    /// This method should only be called after verifying authorization via
    /// `crate::authorization::verify_authorization` with `Role::Operator`.
    /// Direct calls bypass security and should be limited to tests.
    #[cfg(test)]
    pub fn reset(&self) {
        self.last_failure_time.store(u64::MAX, Ordering::Release);
        self.transition_to_closed();
    }

    /// Authorized force open - requires valid operator signature
    pub fn authorized_force_open(
        &self,
        auth: &crate::authorization::AuthorizedCall,
        registry: &mut crate::authorization::OperatorRegistry,
    ) -> Result<(), crate::authorization::AuthError> {
        use crate::authorization::{verify_authorization, Operation, Role};

        // Verify this is a ForceCircuitOpen operation
        if !matches!(auth.operation, Operation::ForceCircuitOpen) {
            return Err(crate::authorization::AuthError::MalformedSignature {
                reason: "operation mismatch".into(),
            });
        }

        verify_authorization(auth, registry, Role::Operator)?;
        self.transition_to_open();
        Ok(())
    }

    /// Authorized force close - requires valid operator signature
    pub fn authorized_force_close(
        &self,
        auth: &crate::authorization::AuthorizedCall,
        registry: &mut crate::authorization::OperatorRegistry,
    ) -> Result<(), crate::authorization::AuthError> {
        use crate::authorization::{verify_authorization, Operation, Role};

        if !matches!(auth.operation, Operation::ForceCircuitClosed) {
            return Err(crate::authorization::AuthError::MalformedSignature {
                reason: "operation mismatch".into(),
            });
        }

        verify_authorization(auth, registry, Role::Operator)?;
        self.transition_to_closed();
        Ok(())
    }

    /// Authorized reset - requires valid operator signature
    pub fn authorized_reset(
        &self,
        auth: &crate::authorization::AuthorizedCall,
        registry: &mut crate::authorization::OperatorRegistry,
    ) -> Result<(), crate::authorization::AuthError> {
        use crate::authorization::{verify_authorization, Operation, Role};

        if !matches!(auth.operation, Operation::ResetCircuitBreaker) {
            return Err(crate::authorization::AuthError::MalformedSignature {
                reason: "operation mismatch".into(),
            });
        }

        verify_authorization(auth, registry, Role::Operator)?;
        self.last_failure_time.store(u64::MAX, Ordering::Release);
        self.transition_to_closed();
        Ok(())
    }

    /// Get current failure count
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Acquire)
    }

    /// Get current success count (in half-open state)
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Acquire)
    }

    /// Get time since last failure
    pub fn time_since_last_failure(&self) -> Option<Duration> {
        let stored = self.last_failure_time.load(Ordering::Acquire);
        if stored == u64::MAX {
            None
        } else {
            Some(self.duration_since_offset(stored))
        }
    }

    /// Get time since last state change
    pub fn time_since_state_change(&self) -> Duration {
        self.elapsed_since_state_change()
    }

    fn offset_now(&self) -> u64 {
        let elapsed = self.start_time.elapsed();
        elapsed.as_millis().min(u128::from(u64::MAX)) as u64
    }

    fn duration_since_offset(&self, timestamp: u64) -> Duration {
        let now = self.offset_now();
        Duration::from_millis(now.saturating_sub(timestamp))
    }

    fn elapsed_since_state_change(&self) -> Duration {
        let stored = self.last_state_change.load(Ordering::Acquire);
        self.duration_since_offset(stored)
    }

    fn touch_state_change(&self) {
        self.last_state_change
            .store(self.offset_now(), Ordering::Release);
    }

    fn transition_to_open(&self) {
        self.state
            .store(CircuitState::Open as u8, Ordering::Release);
        self.touch_state_change();
        self.success_count.store(0, Ordering::Release);

        let failure_count = self.failure_count.load(Ordering::Acquire);
        let threshold = self.config.failure_threshold;
        warn!(
            target: "governance::circuit_breaker",
            failure_count = %failure_count,
            threshold = %threshold,
            "Treasury circuit breaker OPENED - too many failures, rejecting requests"
        );
    }

    fn transition_to_half_open(&self) {
        self.state
            .store(CircuitState::HalfOpen as u8, Ordering::Release);
        self.success_count.store(0, Ordering::Release);
        self.touch_state_change();

        let timeout_secs = self.config.timeout_secs;
        info!(
            target: "governance::circuit_breaker",
            timeout_secs = %timeout_secs,
            "Treasury circuit breaker HALF-OPEN - testing recovery, allowing limited requests"
        );
    }

    fn transition_to_closed(&self) {
        self.state
            .store(CircuitState::Closed as u8, Ordering::Release);
        self.failure_count.store(0, Ordering::Release);
        let prev_success_count = self.success_count.swap(0, Ordering::AcqRel);
        self.touch_state_change();

        info!(
            target: "governance::circuit_breaker",
            success_count = %prev_success_count,
            "Treasury circuit breaker CLOSED - service recovered, normal operation resumed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_initial_state_is_closed() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_opens_after_threshold_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Record failures
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 1);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 2);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert_eq!(cb.failure_count(), 3);

        // Should reject requests
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_success_resets_failure_count_when_closed() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_transitions_to_half_open_after_timeout() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout_secs: 1, // Short timeout for testing
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        thread::sleep(Duration::from_secs(2));

        // Should transition to half-open on next request
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_closes_after_successes_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_secs: 0,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Open circuit
        cb.force_open();
        assert_eq!(cb.state(), CircuitState::Open);

        // Transition to half-open
        cb.state
            .store(CircuitState::HalfOpen as u8, Ordering::Release);

        // Record successes
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert_eq!(cb.success_count(), 1);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.success_count(), 0);
    }

    #[test]
    fn test_reopens_on_failure_in_half_open() {
        let cb = CircuitBreaker::default();

        // Force to half-open
        cb.state
            .store(CircuitState::HalfOpen as u8, Ordering::Release);
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Any failure reopens
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset_clears_state() {
        let cb = CircuitBreaker::default();

        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 3);

        cb.reset();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_force_open_and_close() {
        let cb = CircuitBreaker::default();

        cb.force_open();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());

        cb.force_close();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_time_tracking() {
        let cb = CircuitBreaker::default();

        assert!(cb.time_since_last_failure().is_none());

        cb.record_failure();
        assert!(cb.time_since_last_failure().is_some());

        let elapsed = cb.time_since_state_change();
        assert!(elapsed.as_millis() < 100);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;

        let cb = Arc::new(CircuitBreaker::default());
        let mut handles = vec![];

        // Spawn threads that record failures
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    cb_clone.record_failure();
                }
            }));
        }

        // Spawn threads that record successes
        for _ in 0..5 {
            let cb_clone = Arc::clone(&cb);
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    cb_clone.record_success();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should end up in some valid state
        let state = cb.state();
        assert!(matches!(
            state,
            CircuitState::Closed | CircuitState::Open | CircuitState::HalfOpen
        ));
    }
}
