#![forbid(unsafe_code)]

use crate::{ComputeJobRecord, Explorer};
use anyhow::Result;

/// List compute jobs indexed by the explorer.
pub fn list_jobs(exp: &Explorer) -> Result<Vec<ComputeJobRecord>> {
    exp.compute_jobs()
}
