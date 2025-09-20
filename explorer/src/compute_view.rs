#![forbid(unsafe_code)]

use crate::{ComputeJobRecord, Explorer, ProviderSettlementRecord};
use anyhow::Result;

/// List compute jobs indexed by the explorer.
pub fn list_jobs(exp: &Explorer) -> Result<Vec<ComputeJobRecord>> {
    Ok(exp.compute_jobs()?)
}

/// Return provider settlement balances from the explorer database.
pub fn provider_balances(exp: &Explorer) -> Result<Vec<ProviderSettlementRecord>> {
    Ok(exp.settlement_balances()?)
}
