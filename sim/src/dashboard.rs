#![forbid(unsafe_code)]

use serde::Serialize;

/// Snapshot of simulation state exported for dashboards.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub step: u64,
    pub credits: f64,
    pub supply: f64,
    pub liquidity: f64,
    pub bridged: f64,
    pub consumer_demand: f64,
    pub industrial_demand: f64,
}
