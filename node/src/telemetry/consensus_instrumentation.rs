//! Consensus instrumentation helpers
//!
//! Provides convenience functions to instrument consensus code with telemetry metrics.
//! These should be called from the main consensus loop, block validation paths,
//! and network handlers.

use super::consensus_metrics::*;
use std::time::Instant;

/// RAII guard for measuring block proposal time
pub struct BlockProposalTimer {
    start: Instant,
}

impl BlockProposalTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Drop for BlockProposalTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        BLOCK_PROPOSAL_TIME.observe(elapsed.as_secs_f64());
    }
}

/// RAII guard for measuring block validation time
pub struct BlockValidationTimer {
    start: Instant,
}

impl BlockValidationTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Drop for BlockValidationTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        BLOCK_VALIDATION_TIME.observe(elapsed.as_secs_f64());
    }
}

/// RAII guard for measuring transaction processing time
pub struct TransactionProcessingTimer {
    start: Instant,
}

impl TransactionProcessingTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Drop for TransactionProcessingTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        TRANSACTION_PROCESSING_TIME.observe(elapsed.as_secs_f64());
    }
}

/// RAII guard for measuring receipt validation time
pub struct ReceiptValidationTimer {
    start: Instant,
}

impl ReceiptValidationTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Drop for ReceiptValidationTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        RECEIPT_VALIDATION_TIME.observe(elapsed.as_secs_f64());
    }
}

/// RAII guard for measuring storage proof validation time
pub struct StorageProofValidationTimer {
    start: Instant,
}

impl StorageProofValidationTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Drop for StorageProofValidationTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        STORAGE_PROOF_VALIDATION_TIME.observe(elapsed.as_secs_f64());
    }
}

/// Update block height metric
pub fn set_block_height(height: u64) {
    BLOCK_HEIGHT.set(height as i64);
}

/// Update active peer count
pub fn set_peer_count(count: usize) {
    ACTIVE_PEERS.set(count as i64);
}

/// Record peer latency measurement
pub fn record_peer_latency(duration_secs: f64) {
    PEER_LATENCY.observe(duration_secs);
}

/// Update mempool size
pub fn set_mempool_size(size: usize) {
    MEMPOOL_SIZE.set(size as i64);
}

/// Record fork detection
pub fn record_fork_detected() {
    FORK_DETECTED.inc();
}

/// Update transactions per second
pub fn set_tps(transactions_per_sec: u64) {
    TRANSACTIONS_PER_SECOND.set(transactions_per_sec as i64);
}

/// Update finality lag
pub fn set_finality_lag(blocks_behind: u64) {
    FINALITY_LAG.set(blocks_behind as i64);
}

/// Record network partition detection
pub fn record_network_partition() {
    NETWORK_PARTITION_DETECTED.inc();
}

/// Record consensus stall
pub fn record_consensus_stall() {
    CONSENSUS_STALLED.inc();
}

/// Record orphaned block
pub fn record_orphaned_block() {
    ORPHANED_BLOCKS.inc();
}

/// Record failed receipt validation
pub fn record_receipt_validation_failure() {
    RECEIPT_VALIDATION_FAILURES.inc();
}

/// Record failed storage proof validation
pub fn record_storage_proof_validation_failure() {
    STORAGE_PROOF_VALIDATION_FAILURES.inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_dont_panic() {
        let _block_timer = BlockProposalTimer::new();
        let _validation_timer = BlockValidationTimer::new();
        let _tx_timer = TransactionProcessingTimer::new();
        let _receipt_timer = ReceiptValidationTimer::new();
        let _proof_timer = StorageProofValidationTimer::new();
        // Timers drop and record metrics
    }

    #[test]
    fn helpers_dont_panic() {
        set_block_height(100);
        set_peer_count(50);
        record_peer_latency(0.025);
        set_mempool_size(1000);
        record_fork_detected();
        set_tps(5000);
        set_finality_lag(10);
        record_network_partition();
        record_consensus_stall();
        record_orphaned_block();
        record_receipt_validation_failure();
        record_storage_proof_validation_failure();
    }
}
