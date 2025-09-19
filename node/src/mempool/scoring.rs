use std::collections::VecDeque;

#[cfg(feature = "telemetry")]
use crate::telemetry::{FEE_FLOOR_CURRENT, MEMPOOL_EVICTIONS_TOTAL};

/// Rolling fee floor based on recent transactions.
pub struct FeeFloor {
    window: VecDeque<u64>,
    size: usize,
    percentile: u32,
}

impl FeeFloor {
    pub fn new(size: usize, percentile: u32) -> Self {
        let size = size.max(1);
        let percentile = percentile.min(100);
        Self {
            window: VecDeque::with_capacity(size),
            size,
            percentile,
        }
    }

    fn compute_floor(&self) -> u64 {
        if self.window.is_empty() {
            return 0;
        }
        let mut v: Vec<u64> = self.window.iter().copied().collect();
        v.sort_unstable();
        let mut idx = (v.len() * self.percentile as usize) / 100;
        if idx >= v.len() {
            idx = v.len() - 1;
        }
        v[idx]
    }

    /// Record a fee and return the current floor.
    pub fn update(&mut self, fee: u64) -> u64 {
        if self.window.len() == self.size {
            self.window.pop_front();
        }
        self.window.push_back(fee);
        let floor = self.compute_floor();
        #[cfg(feature = "telemetry")]
        FEE_FLOOR_CURRENT.set(floor as i64);
        floor
    }

    /// Reconfigure the rolling window and percentile. Returns true if the policy changed.
    pub fn configure(&mut self, size: usize, percentile: u32) -> bool {
        let mut changed = false;
        let new_size = size.max(1);
        if self.size != new_size {
            self.size = new_size;
            while self.window.len() > self.size {
                self.window.pop_front();
            }
            if self.window.capacity() < self.size {
                self.window.reserve(self.size - self.window.capacity());
            }
            changed = true;
        }
        let new_percentile = percentile.min(100);
        if self.percentile != new_percentile {
            self.percentile = new_percentile;
            changed = true;
        }
        if changed {
            #[cfg(feature = "telemetry")]
            {
                let floor = self.compute_floor();
                FEE_FLOOR_CURRENT.set(floor as i64);
            }
        }
        changed
    }

    /// Return the current fee floor without mutating the window.
    pub fn current(&self) -> u64 {
        self.compute_floor()
    }

    /// Return the configured policy (window size, percentile).
    pub fn policy(&self) -> (usize, u32) {
        (self.size, self.percentile)
    }
}

/// Evict lowest-fee transactions when the mempool is full.
pub fn evict_on_overflow(count: usize) {
    if count > 0 {
        #[cfg(feature = "telemetry")]
        MEMPOOL_EVICTIONS_TOTAL.inc_by(count as u64);
    }
}
