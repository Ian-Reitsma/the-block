#![forbid(unsafe_code)]

use foundation_serialization::json;
use foundation_serialization::Serialize;
use rand::{rngs::StdRng, Rng};

pub mod bridging;
pub mod dashboard;
pub mod demand;
pub mod dex;
pub mod inflation;
pub mod liquidity;
#[cfg(feature = "runtime-wrapper")]
pub mod mobile_sync;
pub mod token_model;

#[cfg(feature = "dependency-fault")]
pub mod dependency_fault_harness;

use bridging::BridgeModel;
use dashboard::Snapshot;
use demand::DemandModel;
use inflation::InflationModel;
use liquidity::LiquidityModel;

/// Storage backend selection for reproducible scenarios.
#[derive(Clone, Copy)]
pub enum Backend {
    Memory,
    LegacyRocksDb,
}

/// Economic and network simulation harness.
pub struct Simulation {
    pub nodes: u64,
    pub subsidy: f64,
    pub inflation: InflationModel,
    pub liquidity: LiquidityModel,
    pub bridging: BridgeModel,
    pub demand: DemandModel,
    pub backlog: f64,
    pub backend: Backend,
    rng: StdRng,
    partition_steps: u64,
    reconciliation_latency: u64,
    session_keys: u64,
    session_expired: u64,
    wasm_exec: u64,
}

impl Simulation {
    /// Create a new simulation with default economic models.
    pub fn new(nodes: u64) -> Self {
        let seed = std::env::var("TB_SIM_SEED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(42);
        Self {
            nodes,
            subsidy: 0.0,
            inflation: InflationModel::default(),
            liquidity: LiquidityModel::default(),
            bridging: BridgeModel::default(),
            demand: DemandModel::default(),
            backlog: 0.0,
            backend: Backend::Memory,
            rng: StdRng::seed_from_u64(seed),
            partition_steps: 0,
            reconciliation_latency: 0,
            session_keys: 0,
            session_expired: 0,
            wasm_exec: 0,
        }
    }

    /// Create a simulation with an explicit storage backend.
    pub fn with_backend(nodes: u64, backend: Backend) -> Self {
        let mut sim = Self::new(nodes);
        if let Backend::LegacyRocksDb = backend {
            panic!(
                "legacy RocksDB backend has been removed; rerun with Backend::Memory or the \
                 forthcoming in-house engine"
            );
        }
        sim.backend = backend;
        sim
    }

    /// Start a simulated network partition lasting `steps` steps.
    pub fn start_partition(&mut self, steps: u64) {
        self.partition_steps = steps;
        self.reconciliation_latency = steps;
    }

    /// Run the simulation for the given number of steps and write a CSV dashboard.
    pub fn run(&mut self, steps: u64, out: &str) -> csv::Result<()> {
        let mut wtr = csv::Writer::from_path(out)?;
        for step in 0..steps {
            let snap = self.step(step);
            wtr.serialize(&snap)?;
        }
        wtr.flush().map_err(csv::Error::from)
    }

    /// Export aggregated state to a governance decision template (JSON).
    pub fn export_governance<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        #[derive(Serialize)]
        struct Governance<'a> {
            total_subsidy: f64,
            total_supply: f64,
            liquidity: f64,
            bridged: f64,
            consumer_demand: f64,
            industrial_demand: f64,
            nodes: u64,
            _p: std::marker::PhantomData<&'a ()>,
        }
        let g = Governance {
            total_subsidy: self.subsidy,
            total_supply: self.inflation.supply,
            liquidity: self.liquidity.token_reserve,
            bridged: self.bridging.bridged,
            consumer_demand: self.demand.consumer_growth,
            industrial_demand: self.demand.industrial_growth,
            nodes: self.nodes,
            _p: std::marker::PhantomData,
        };
        let data = json::to_vec_pretty(&g).expect("serialize governance");
        std::fs::write(path, data)
    }

    /// Advance the simulation by one step.
    pub fn step(&mut self, step: u64) -> Snapshot {
        let inc: f64 = self.rng.gen_range(0.0..1.0);
        self.subsidy += inc;
        let supply = self.inflation.apply(inc);
        let liquidity = self.liquidity.update(inc);
        if self.partition_steps > 0 {
            self.partition_steps -= 1;
        }
        let bridged = self.bridging.flow(inc);
        let (consumer_demand, industrial_demand) = self.demand.project();
        let total_demand = consumer_demand + industrial_demand;
        if liquidity < total_demand {
            self.backlog += total_demand - liquidity;
        } else {
            let diff = liquidity - total_demand;
            self.backlog = (self.backlog - diff).max(0.0);
        }
        // simple session-key churn proportional to demand
        let churn = (total_demand * 10.0) as u64;
        self.session_keys += churn;
        if self.rng.gen_bool(0.1) && self.session_keys > 0 {
            self.session_keys -= 1;
            self.session_expired += 1;
        }
        // simulate heavy wasm contract executions
        self.wasm_exec = self.rng.gen_range(0..1000);
        let inflation_rate = self.inflation.rate;
        let sell_coverage = if self.backlog == 0.0 {
            1.0
        } else {
            self.liquidity.token_reserve / self.backlog.max(1.0)
        };
        let readiness = 1.0 / (1.0 + self.backlog);
        let partition_active = self.partition_steps > 0;
        let snap = Snapshot {
            step,
            subsidy: self.subsidy,
            supply,
            liquidity,
            bridged,
            consumer_demand,
            industrial_demand,
            backlog: self.backlog,
            inflation_rate,
            sell_coverage,
            readiness,
            partition_active,
            reconciliation_latency: self.reconciliation_latency,
            active_sessions: self.session_keys,
            expired_sessions: self.session_expired,
            wasm_exec: self.wasm_exec,
        };
        if !partition_active && self.reconciliation_latency > 0 {
            self.reconciliation_latency -= 1;
        }
        #[cfg(feature = "sim-db")]
        if let Some(db) = &self.db {
            let key = step.to_be_bytes();
            let val = bincode::serialize(&snap).expect("serialize snapshot");
            let _ = db.put(key, val);
        }
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflation_increases_supply() {
        let mut sim = Simulation::new(1);
        let before = sim.inflation.supply;
        sim.step(0);
        assert!(sim.inflation.supply > before);
    }
}
