#![forbid(unsafe_code)]

use crate::{DidRecordRow, Explorer, MetricPoint};
use anyhow::Result;

/// Return the most recent DID anchors persisted by the explorer.
pub fn recent(exp: &Explorer, limit: usize) -> Result<Vec<DidRecordRow>> {
    exp.recent_did_records(limit)
}

/// Fetch the anchor history for a specific address.
pub fn by_address(exp: &Explorer, address: &str) -> Result<Vec<DidRecordRow>> {
    exp.did_records_for_address(address)
}

/// Compute the per-second DID anchor rate from the archived counter metric.
pub fn anchor_rate(exp: &Explorer) -> Result<Vec<MetricPoint>> {
    exp.did_anchor_rate()
}
