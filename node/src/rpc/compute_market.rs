use serde_json::json;

use crate::compute_market::{
    price_board, scheduler, settlement::Settlement, total_units_processed,
};
use crate::transaction::FeeLane;

/// Return compute market backlog and utilisation metrics.
pub fn stats(_accel: Option<crate::compute_market::Accelerator>) -> serde_json::Value {
    let (backlog, util) = price_board::backlog_utilization();
    let weighted = price_board::bands(FeeLane::Industrial).map(|(_, m, _)| m);
    let raw = price_board::raw_bands(FeeLane::Industrial).map(|(_, m, _)| m);
    let spot = price_board::spot_price_per_unit(FeeLane::Industrial)
        .or(weighted)
        .or(raw)
        .unwrap_or_default();
    let sched = scheduler::stats();
    json!({
        "industrial_backlog": backlog,
        "industrial_utilization": util,
        "industrial_units_total": total_units_processed(),
        "industrial_price_per_unit": spot,
        "industrial_price_weighted": weighted,
        "industrial_price_base": raw,
        "pending": sched.pending,
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

/// Return capability requirements for an active job.
pub fn job_requirements(job_id: &str) -> serde_json::Value {
    if let Some(cap) = scheduler::job_requirements(job_id) {
        serde_json::to_value(cap).unwrap()
    } else {
        json!({})
    }
}

/// Cancel an active job and release resources.
pub fn job_cancel(job_id: &str) -> serde_json::Value {
    if let Some(provider) = scheduler::active_provider(job_id) {
        scheduler::cancel_job(job_id, &provider, scheduler::CancelReason::Client);
        json!({ "status": "ok" })
    } else {
        json!({ "status": "unknown" })
    }
}

/// Return advertised hardware capability for a provider.
pub fn provider_hardware(provider: &str) -> serde_json::Value {
    if let Some(cap) = scheduler::provider_capability(provider) {
        serde_json::to_value(cap).unwrap()
    } else {
        json!({})
    }
}

/// Return the recent settlement audit log.
pub fn settlement_audit() -> serde_json::Value {
    serde_json::to_value(Settlement::audit()).unwrap_or_else(|_| json!([]))
}

/// Return split token balances for providers.
pub fn provider_balances() -> serde_json::Value {
    json!({ "providers": Settlement::balances() })
}

/// Return recent settlement merkle roots encoded as hex strings.
pub fn recent_roots(limit: usize) -> serde_json::Value {
    let roots: Vec<String> = Settlement::recent_roots(limit)
        .into_iter()
        .map(|r| hex::encode(r))
        .collect();
    json!({ "roots": roots })
}
