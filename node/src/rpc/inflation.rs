use std::sync::{Arc, Mutex};

use foundation_serialization::json::json;

use crate::{compute_market::price_board, Blockchain};

/// Return current subsidy multipliers and industrial demand metrics.
pub fn params(bc: &Arc<Mutex<Blockchain>>) -> foundation_serialization::json::Value {
    let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
    let (backlog, util) = price_board::backlog_utilization();
    json!({
        "beta_storage_sub_ct": guard.params.beta_storage_sub_ct,
        "gamma_read_sub_ct": guard.params.gamma_read_sub_ct,
        "kappa_cpu_sub_ct": guard.params.kappa_cpu_sub_ct,
        "lambda_bytes_out_sub_ct": guard.params.lambda_bytes_out_sub_ct,
        "industrial_multiplier": guard.params.industrial_multiplier,
        "industrial_backlog": backlog,
        "industrial_utilization": util,
        "rent_rate_ct_per_byte": guard.params.rent_rate_ct_per_byte,
    })
}
