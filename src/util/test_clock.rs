use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::clock::Clock;

/// Clock that only advances when instructed, useful for deterministic tests.
#[derive(Clone)]
pub struct PausedClock {
    inner: Arc<Mutex<Instant>>,
}

impl PausedClock {
    /// Create a new paused clock starting at `start`.
    pub fn new(start: Instant) -> Self {
        Self {
            inner: Arc::new(Mutex::new(start)),
        }
    }

    /// Manually advance the clock by `delta`.
    pub fn advance(&self, delta: Duration) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = *guard + delta;
    }
}

impl Clock for PausedClock {
    fn now(&self) -> Instant {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}
