#![forbid(unsafe_code)]

use crate::{ComputeJobRecord, Explorer, ProviderSettlementRecord};
use diagnostics::anyhow::{self, Result};

/// List compute jobs indexed by the explorer.
pub fn list_jobs(exp: &Explorer) -> Result<Vec<ComputeJobRecord>> {
    exp.compute_jobs().map_err(anyhow::Error::from_error)
}

/// Return provider settlement balances from the explorer database.
pub fn provider_balances(exp: &Explorer) -> Result<Vec<ProviderSettlementRecord>> {
    exp.settlement_balances().map_err(anyhow::Error::from_error)
}
