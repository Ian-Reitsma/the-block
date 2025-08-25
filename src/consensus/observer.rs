#![cfg(feature = "telemetry")]

use std::time::Instant;

use crate::telemetry::{FORK_REORG_TOTAL, GOSSIP_CONVERGENCE_SECONDS};

/// Record the time it took for peers to converge on a common tip.
pub fn observe_convergence(start: Instant) {
    let secs = start.elapsed().as_secs_f64();
    GOSSIP_CONVERGENCE_SECONDS.observe(secs);
}

/// Increment the reorg counter for a given depth.
pub fn record_reorg(depth: u64) {
    FORK_REORG_TOTAL
        .with_label_values(&[&depth.to_string()])
        .inc();
}
