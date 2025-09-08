use serde_json::json;

use crate::compute_market::{price_board, scheduler};

/// Return compute market backlog and utilisation metrics.
pub fn stats() -> serde_json::Value {
    let (backlog, util) = price_board::backlog_utilization();
    json!({
        "industrial_backlog": backlog,
        "industrial_utilization": util,
    })
}

/// Return scheduler reputation and capability utilisation metrics.
pub fn scheduler_metrics() -> serde_json::Value {
    scheduler::metrics()
}

/// Return aggregated scheduler statistics over recent matches.
pub fn scheduler_stats() -> serde_json::Value {
    serde_json::to_value(scheduler::stats()).unwrap()
}

/// Return current reputation score for a provider.
pub fn reputation_get(provider: &str) -> serde_json::Value {
    json!({
        "provider": provider,
        "score": scheduler::reputation_get(provider),
    })
}
