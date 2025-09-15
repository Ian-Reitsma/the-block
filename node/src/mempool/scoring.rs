use std::collections::VecDeque;

#[cfg(feature = "telemetry")]
use crate::telemetry::{FEE_FLOOR_CURRENT, MEMPOOL_EVICTIONS_TOTAL};

/// Rolling fee floor based on recent transactions.
pub struct FeeFloor {
    window: VecDeque<u64>,
    size: usize,
}

impl FeeFloor {
    pub fn new(size: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(size),
            size,
        }
    }

    /// Record a fee and return the current floor (75th percentile).
    pub fn update(&mut self, fee: u64) -> u64 {
        if self.window.len() == self.size {
            self.window.pop_front();
        }
        self.window.push_back(fee);
        let mut v: Vec<u64> = self.window.iter().copied().collect();
        v.sort_unstable();
        let idx = (v.len() * 3) / 4;
        let floor = v.get(idx).copied().unwrap_or(0);
        #[cfg(feature = "telemetry")]
        FEE_FLOOR_CURRENT.set(floor as i64);
        floor
    }
}

/// Evict lowest-fee transactions when the mempool is full.
pub fn evict_on_overflow(count: usize) {
    if count > 0 {
        #[cfg(feature = "telemetry")]
        MEMPOOL_EVICTIONS_TOTAL.inc_by(count as u64);
    }
}
