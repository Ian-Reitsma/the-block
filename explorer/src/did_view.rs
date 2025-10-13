#![forbid(unsafe_code)]

use crate::{DidRecordRow, Explorer, MetricPoint};
use diagnostics::anyhow::{self, Result};

/// Return the most recent DID anchors persisted by the explorer.
pub fn recent(exp: &Explorer, limit: usize) -> Result<Vec<DidRecordRow>> {
    exp.recent_did_records(limit)
        .map_err(anyhow::Error::from_error)
}

/// Fetch the anchor history for a specific address.
pub fn by_address(exp: &Explorer, address: &str) -> Result<Vec<DidRecordRow>> {
    exp.did_records_for_address(address)
        .map_err(anyhow::Error::from_error)
}

/// Compute the per-second DID anchor rate from the archived counter metric.
pub fn anchor_rate(exp: &Explorer) -> Result<Vec<MetricPoint>> {
    exp.did_anchor_rate().map_err(anyhow::Error::from_error)
}
