#![forbid(unsafe_code)]

use foundation_serialization::json;
use foundation_serialization::Serialize;
use rand::{rngs::StdRng, Rng};
use std::fs::File;
use std::io::{self, BufWriter, Write};

pub mod bridging;
pub mod chaos;
pub mod dashboard;
pub mod demand;
pub mod dex;
pub mod inflation;
pub mod liquidity;
#[cfg(feature = "runtime-wrapper")]
pub mod mobile_sync;
#[cfg(not(feature = "runtime-wrapper"))]
pub mod mobile_sync {
    use diagnostics::tracing::warn;
    use light_client::Header;
    use std::time::Duration;

    /// Stubbed mobile sync latency measurement used when the runtime wrapper is
    /// unavailable. Returns zero duration while emitting a warning so callers
    /// know the benchmark was skipped.
    pub fn measure_sync_latency(headers: Vec<Header>, _delay: Duration) -> Duration {
        if !headers.is_empty() {
            warn!(target: "sim", "mobile_sync_stub_inactive_runtime");
        }
        Duration::default()
    }
}
pub mod token_model;

#[cfg(feature = "dependency-fault")]
pub mod dependency_fault_harness;

use bridging::BridgeModel;
use chaos::{
    ChaosEvent, ChaosFault, ChaosHarness, ChaosModule, ChaosProviderKind, ChaosScenario, ChaosSite,
};
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
    pub chaos: ChaosHarness,
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
        let mut chaos = ChaosHarness::new();
        chaos.register(
            ChaosScenario::new("overlay-wide-partition", ChaosModule::Overlay, 0.9, 0.12)
                .add_event(ChaosEvent::new(
                    5,
                    12,
                    0.35,
                    ChaosFault::OverlayPartition { loss_ratio: 0.7 },
                ))
                .add_event(ChaosEvent::new(
                    25,
                    8,
                    0.5,
                    ChaosFault::OverlayPartition { loss_ratio: 0.5 },
                )),
        );
        chaos.configure_sites(
            ChaosModule::Overlay,
            vec![
                ChaosSite::with_kind("provider-a", 0.55, 0.1, ChaosProviderKind::Foundation),
                ChaosSite::with_kind("provider-b", 0.45, 0.18, ChaosProviderKind::Partner),
            ],
        );
        chaos.register(
            ChaosScenario::new("storage-dht-failure", ChaosModule::Storage, 0.88, 0.08)
                .add_event(ChaosEvent::new(
                    10,
                    15,
                    0.4,
                    ChaosFault::StorageCorruption { shard_loss: 0.5 },
                ))
                .add_event(ChaosEvent::new(
                    40,
                    10,
                    0.2,
                    ChaosFault::StorageCorruption { shard_loss: 0.3 },
                )),
        );
        chaos.register(
            ChaosScenario::new("compute-backpressure", ChaosModule::Compute, 0.9, 0.1)
                .add_event(ChaosEvent::new(
                    15,
                    20,
                    0.45,
                    ChaosFault::ComputeBackpressure {
                        throttle_ratio: 0.6,
                    },
                ))
                .add_event(ChaosEvent::new(
                    60,
                    15,
                    0.35,
                    ChaosFault::ComputeBackpressure {
                        throttle_ratio: 0.5,
                    },
                )),
        );
        Self {
            nodes,
            subsidy: 0.0,
            inflation: InflationModel::default(),
            liquidity: LiquidityModel::default(),
            bridging: BridgeModel::default(),
            demand: DemandModel::default(),
            backlog: 0.0,
            backend: Backend::Memory,
            chaos,
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

    /// Run the simulation without exporting dashboards.
    pub fn drive(&mut self, steps: u64) {
        for step in 0..steps {
            let _ = self.step(step);
        }
    }

    /// Run the simulation for the given number of steps and write a CSV dashboard.
    pub fn run(&mut self, steps: u64, out: &str) -> io::Result<()> {
        let file = File::create(out)?;
        let mut writer = BufWriter::new(file);
        write_dashboard_header(&mut writer)?;
        for step in 0..steps {
            let snap = self.step(step);
            write_dashboard_row(&mut writer, &snap)?;
        }
        writer.flush()
    }

    /// Export aggregated state to a governance decision template (JSON).
    pub fn export_governance<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        #[derive(Serialize)]
        struct Governance {
            total_subsidy: f64,
            total_supply: f64,
            liquidity: f64,
            bridged: f64,
            consumer_demand: f64,
            industrial_demand: f64,
            nodes: u64,
        }
        let g = Governance {
            total_subsidy: self.subsidy,
            total_supply: self.inflation.supply,
            liquidity: self.liquidity.token_reserve,
            bridged: self.bridging.bridged,
            consumer_demand: self.demand.consumer_growth,
            industrial_demand: self.demand.industrial_growth,
            nodes: self.nodes,
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
        let base_readiness = 1.0 / (1.0 + self.backlog);
        let chaos_report = self.chaos.step(step, &mut self.rng);
        let overlay_readiness = chaos_report.overlay.readiness;
        let storage_readiness = chaos_report.storage.readiness;
        let compute_readiness = chaos_report.compute.readiness;
        let readiness_multiplier = overlay_readiness
            .min(storage_readiness)
            .min(compute_readiness);
        let readiness = base_readiness * readiness_multiplier;
        let chaos_breaches = chaos_report.overlay.breaches
            + chaos_report.storage.breaches
            + chaos_report.compute.breaches;
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
            overlay_readiness,
            storage_readiness,
            compute_readiness,
            chaos_breaches,
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
        {
            // DB snapshot recording disabled - feature not fully implemented
            let _ = step;
            let _ = &snap;
        }
        snap
    }
}

fn write_dashboard_header<W: Write>(writer: &mut W) -> io::Result<()> {
    writer.write_all(b"step,subsidy,supply,liquidity,bridged,consumer_demand,industrial_demand,backlog,inflation_rate,sell_coverage,readiness,overlay_readiness,storage_readiness,compute_readiness,chaos_breaches,partition_active,reconciliation_latency,active_sessions,expired_sessions,wasm_exec\n")
}

fn write_dashboard_row<W: Write>(writer: &mut W, snap: &Snapshot) -> io::Result<()> {
    writeln!(
        writer,
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        snap.step,
        snap.subsidy,
        snap.supply,
        snap.liquidity,
        snap.bridged,
        snap.consumer_demand,
        snap.industrial_demand,
        snap.backlog,
        snap.inflation_rate,
        snap.sell_coverage,
        snap.readiness,
        snap.overlay_readiness,
        snap.storage_readiness,
        snap.compute_readiness,
        snap.chaos_breaches,
        snap.partition_active,
        snap.reconciliation_latency,
        snap.active_sessions,
        snap.expired_sessions,
        snap.wasm_exec
    )
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
