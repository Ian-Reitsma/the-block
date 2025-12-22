//! Consensus health metrics
//!
//! Critical metrics for monitoring consensus layer health:
//! - Block height and proposal time
//! - Network connectivity (peer count, latency)
//! - Fork detection and finality
//! - Mempool depth and transaction processing

use crate::telemetry::{register_counter, register_gauge, register_histogram, register_int_gauge};
use concurrency::Lazy;
use runtime::telemetry::{Histogram, IntCounter, IntGauge};

/// Current block height (chain tip)
pub static BLOCK_HEIGHT: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge("block_height", "Current blockchain height"));

/// Number of active connected peers
pub static ACTIVE_PEERS: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge("active_peers_total", "Number of connected peers"));

/// Peer round-trip time distribution
pub static PEER_LATENCY: Lazy<Histogram> =
    Lazy::new(|| register_histogram("peer_latency_seconds", "Peer round-trip time in seconds"));

/// Time to propose a new block
pub static BLOCK_PROPOSAL_TIME: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "block_proposal_seconds",
        "Time to assemble and propose a block",
    )
});

/// Fork detection events
pub static FORK_DETECTED: Lazy<IntCounter> =
    Lazy::new(|| register_counter("forks_detected_total", "Number of chain forks detected"));

/// Number of transactions in mempool
pub static MEMPOOL_SIZE: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge("mempool_transactions", "Pending transactions in mempool"));

/// Transaction validation time
pub static TRANSACTION_PROCESSING_TIME: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "transaction_processing_seconds",
        "Time to validate a transaction",
    )
});

/// Block validation time
pub static BLOCK_VALIDATION_TIME: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "block_validation_seconds",
        "Time to validate a received block",
    )
});

/// Transactions per second (rolling window)
pub static TRANSACTIONS_PER_SECOND: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge("transactions_per_second", "Current transaction throughput"));

/// Finality lag (blocks behind chain tip)
pub static FINALITY_LAG: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge(
        "finality_lag_blocks",
        "Blocks between chain tip and finalized height",
    )
});

/// Network partition detected
pub static NETWORK_PARTITION_DETECTED: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "network_partition_detected_total",
        "Network partition events detected",
    )
});

/// Consensus stalls (no new blocks in expected window)
pub static CONSENSUS_STALLED: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "consensus_stalled_total",
        "Consensus stall events (no blocks in 2+ minutes)",
    )
});

/// Orphaned blocks
pub static ORPHANED_BLOCKS: Lazy<IntCounter> =
    Lazy::new(|| register_counter("orphaned_blocks_total", "Blocks that became orphaned"));

/// Receipt validation time
pub static RECEIPT_VALIDATION_TIME: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "receipt_validation_seconds",
        "Time to validate a receipt including signature",
    )
});

/// Failed receipt validations
pub static RECEIPT_VALIDATION_FAILURES: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "receipt_validation_failures_total",
        "Number of receipts that failed validation",
    )
});

/// Storage proof validation time
pub static STORAGE_PROOF_VALIDATION_TIME: Lazy<Histogram> = Lazy::new(|| {
    register_histogram(
        "storage_proof_validation_seconds",
        "Time to validate a storage Merkle proof",
    )
});

/// Failed storage proof validations
pub static STORAGE_PROOF_VALIDATION_FAILURES: Lazy<IntCounter> = Lazy::new(|| {
    register_counter(
        "storage_proof_validation_failures_total",
        "Number of storage proofs that failed validation",
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_initialize() {
        // Ensure all metrics can be created without panicking
        let _ = &*BLOCK_HEIGHT;
        let _ = &*ACTIVE_PEERS;
        let _ = &*PEER_LATENCY;
        let _ = &*BLOCK_PROPOSAL_TIME;
        let _ = &*FORK_DETECTED;
        let _ = &*MEMPOOL_SIZE;
        let _ = &*TRANSACTION_PROCESSING_TIME;
        let _ = &*BLOCK_VALIDATION_TIME;
        let _ = &*TRANSACTIONS_PER_SECOND;
        let _ = &*FINALITY_LAG;
        let _ = &*NETWORK_PARTITION_DETECTED;
        let _ = &*CONSENSUS_STALLED;
        let _ = &*ORPHANED_BLOCKS;
        let _ = &*RECEIPT_VALIDATION_TIME;
        let _ = &*RECEIPT_VALIDATION_FAILURES;
        let _ = &*STORAGE_PROOF_VALIDATION_TIME;
        let _ = &*STORAGE_PROOF_VALIDATION_FAILURES;
    }
}
