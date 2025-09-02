use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Minimal placeholder scheduler aggregating blob roots before anchoring.
pub struct BlobScheduler {
    l2_queue: VecDeque<[u8; 32]>,
    l3_queue: VecDeque<[u8; 32]>,
    last_l2: Instant,
    last_l3: Instant,
}

impl BlobScheduler {
    pub fn new() -> Self {
        Self {
            l2_queue: VecDeque::new(),
            l3_queue: VecDeque::new(),
            last_l2: Instant::now(),
            last_l3: Instant::now(),
        }
    }

    /// Enqueue a blob root targeting L2 (≤4 GB) or L3 (>4 GB).
    pub fn push(&mut self, root: [u8; 32], is_l3: bool) {
        if is_l3 {
            self.l3_queue.push_back(root);
        } else {
            self.l2_queue.push_back(root);
        }
    }

    /// Retrieve L2 roots when the 4‑s cadence elapses.
    pub fn pop_l2_ready(&mut self) -> Vec<[u8; 32]> {
        if self.last_l2.elapsed() >= Duration::from_secs(4) {
            self.last_l2 = Instant::now();
            self.l2_queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Retrieve L3 roots when the 16‑s cadence elapses.
    pub fn pop_l3_ready(&mut self) -> Vec<[u8; 32]> {
        if self.last_l3.elapsed() >= Duration::from_secs(16) {
            self.last_l3 = Instant::now();
            self.l3_queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for BlobScheduler {
    fn default() -> Self { Self::new() }
}
