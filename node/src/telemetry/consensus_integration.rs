//! Final consensus integration layer
//!
//! This module provides a single high-level integration point for all consensus
//! telemetry instrumentation. Wire metrics updates here during block processing.

use super::consensus_instrumentation::*;
use super::consensus_metrics::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Instant;

/// Consensus state tracker for efficient metric updates
pub struct ConsensusStateTracker {
    last_block_time: Arc<AtomicU64>,
    last_block_height: Arc<AtomicU64>,
    stall_threshold_secs: u64,
}

impl ConsensusStateTracker {
    /// Create new consensus tracker with 2-minute stall threshold
    pub fn new() -> Self {
        Self {
            last_block_time: Arc::new(AtomicU64::new(0)),
            last_block_height: Arc::new(AtomicU64::new(0)),
            stall_threshold_secs: 120,
        }
    }

    /// Record block application and update all core metrics
    ///
    /// This is the primary integration point. Call after successfully applying
    /// and committing a block.
    pub fn record_block_applied(
        &self,
        height: u64,
        tx_count: usize,
        finalized_height: u64,
        now_secs: u64,
    ) {
        // Update height metric
        set_block_height(height);

        // Update throughput (exclude coinbase/system tx)
        let user_txs = tx_count.saturating_sub(1);
        set_tps(user_txs as u64);

        // Update finality lag
        if height >= finalized_height {
            set_finality_lag(height - finalized_height);
        }

        // Track time for stall detection
        let last_time = self.last_block_time.load(AtomicOrdering::Relaxed);
        self.last_block_time
            .store(now_secs, AtomicOrdering::Release);
        self.last_block_height
            .store(height, AtomicOrdering::Release);

        // Detect consensus stalls (no blocks in threshold time)
        if last_time > 0 && now_secs > last_time {
            let elapsed = now_secs - last_time;
            if elapsed > self.stall_threshold_secs {
                record_consensus_stall();
            }
        }
    }

    /// Update network peer metrics
    pub fn update_peer_metrics(&self, peer_count: usize, avg_latency_secs: f64) {
        set_peer_count(peer_count);
        if avg_latency_secs > 0.0 {
            record_peer_latency(avg_latency_secs);
        }
    }

    /// Update mempool metrics
    pub fn update_mempool_metrics(&self, pending_tx_count: usize) {
        set_mempool_size(pending_tx_count);
    }

    /// Record fork detection event
    pub fn record_fork(&self) {
        super::consensus_metrics::FORK_DETECTED.inc();
    }

    /// Record orphaned block
    pub fn record_orphan(&self) {
        record_orphaned_block();
    }

    /// Record network partition detection
    pub fn record_partition(&self) {
        super::consensus_metrics::NETWORK_PARTITION_DETECTED.inc();
    }

    /// Get current tracked height
    pub fn current_height(&self) -> u64 {
        self.last_block_height.load(AtomicOrdering::Acquire)
    }

    /// Get current tracked time
    pub fn current_time(&self) -> u64 {
        self.last_block_time.load(AtomicOrdering::Acquire)
    }
}

impl Default for ConsensusStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// High-performance metric batch recorder
///
/// For processing multiple blocks in sequence, use batching to reduce
/// atomic operations overhead.
pub struct MetricBatcher {
    blocks_applied: u64,
    total_txs: u64,
    max_finality_lag: u64,
    forks_detected: u64,
    orphans_detected: u64,
}

impl MetricBatcher {
    pub fn new() -> Self {
        Self {
            blocks_applied: 0,
            total_txs: 0,
            max_finality_lag: 0,
            forks_detected: 0,
            orphans_detected: 0,
        }
    }

    pub fn add_block(&mut self, tx_count: usize, finality_lag: u64) {
        self.blocks_applied += 1;
        self.total_txs += tx_count as u64;
        self.max_finality_lag = self.max_finality_lag.max(finality_lag);
    }

    pub fn add_fork(&mut self) {
        self.forks_detected += 1;
    }

    pub fn add_orphan(&mut self) {
        self.orphans_detected += 1;
    }

    /// Flush all batched metrics at once
    pub fn flush(&self, current_height: u64, current_finalized: u64) {
        if self.blocks_applied > 0 {
            let avg_tps = self.total_txs / self.blocks_applied;
            set_tps(avg_tps);
            set_block_height(current_height);

            if current_height >= current_finalized {
                set_finality_lag(current_height - current_finalized);
            }
        }

        for _ in 0..self.forks_detected {
            super::consensus_metrics::FORK_DETECTED.inc();
        }

        for _ in 0..self.orphans_detected {
            record_orphaned_block();
        }
    }
}

impl Default for MetricBatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_tracker_initializes() {
        let tracker = ConsensusStateTracker::new();
        assert_eq!(tracker.current_height(), 0);
        assert_eq!(tracker.current_time(), 0);
    }

    #[test]
    fn state_tracker_records_block() {
        let tracker = ConsensusStateTracker::new();
        tracker.record_block_applied(100, 50, 95, 1000);
        assert_eq!(tracker.current_height(), 100);
        assert_eq!(tracker.current_time(), 1000);
    }

    #[test]
    fn metric_batcher_accumulates() {
        let mut batcher = MetricBatcher::new();
        batcher.add_block(10, 5);
        batcher.add_block(15, 3);
        batcher.add_fork();
        assert_eq!(batcher.blocks_applied, 2);
        assert_eq!(batcher.total_txs, 25);
        assert_eq!(batcher.forks_detected, 1);
    }

    #[test]
    fn batcher_computes_avg_tps() {
        let mut batcher = MetricBatcher::new();
        batcher.add_block(10, 0);
        batcher.add_block(20, 0);
        // Average should be 15 TPS
        assert_eq!(batcher.total_txs / batcher.blocks_applied, 15);
    }
}
