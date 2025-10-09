#![forbid(unsafe_code)]

use foundation_serialization::Serialize;

/// Snapshot of simulation state exported for dashboards.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub step: u64,
    pub subsidy: f64,
    pub supply: f64,
    pub liquidity: f64,
    pub bridged: f64,
    pub consumer_demand: f64,
    pub industrial_demand: f64,
    pub backlog: f64,
    pub inflation_rate: f64,
    pub sell_coverage: f64,
    pub readiness: f64,
    pub partition_active: bool,
    pub reconciliation_latency: u64,
    pub active_sessions: u64,
    pub expired_sessions: u64,
    pub wasm_exec: u64,
}
