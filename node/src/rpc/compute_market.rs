use serde_json::json;

use crate::compute_market::price_board;

/// Return compute market backlog and utilisation metrics.
pub fn stats() -> serde_json::Value {
    let (backlog, util) = price_board::backlog_utilization();
    json!({
        "industrial_backlog": backlog,
        "industrial_utilization": util,
    })
}

