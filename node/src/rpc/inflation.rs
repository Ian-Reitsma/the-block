use std::sync::{Arc, Mutex};

use crate::{compute_market::price_board, Blockchain};
use foundation_serialization::Serialize;

/// Return current subsidy multipliers and industrial demand metrics.
#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationParamsResponse {
    pub beta_storage_sub: i64,
    pub gamma_read_sub: i64,
    pub kappa_cpu_sub: i64,
    pub lambda_bytes_out_sub: i64,
    pub industrial_multiplier: i64,
    pub industrial_backlog: u64,
    pub industrial_utilization: u64,
    pub rent_rate_per_byte: i64,
}

pub fn params(bc: &Arc<Mutex<Blockchain>>) -> InflationParamsResponse {
    let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
    let (backlog, util) = price_board::backlog_utilization();
    InflationParamsResponse {
        beta_storage_sub: guard.params.beta_storage_sub,
        gamma_read_sub: guard.params.gamma_read_sub,
        kappa_cpu_sub: guard.params.kappa_cpu_sub,
        lambda_bytes_out_sub: guard.params.lambda_bytes_out_sub,
        industrial_multiplier: guard.params.industrial_multiplier,
        industrial_backlog: backlog,
        industrial_utilization: util,
        rent_rate_per_byte: guard.params.rent_rate_per_byte,
    }
}
