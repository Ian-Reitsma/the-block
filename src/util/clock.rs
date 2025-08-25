use std::time::Instant;

/// Abstraction over time to enable deterministic tests.
pub trait Clock: Clone + Send + Sync + 'static {
    /// Return the current instant.
    fn now(&self) -> Instant;
}

/// Clock implementation using `Instant::now()` for production.
#[derive(Clone, Default)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
