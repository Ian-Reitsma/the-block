#![forbid(unsafe_code)]

use rand::Rng;
use serde::Serialize;

pub mod bridging;
pub mod dashboard;
pub mod demand;
pub mod inflation;
pub mod liquidity;
pub mod dex;

use bridging::BridgeModel;
use dashboard::Snapshot;
use demand::DemandModel;
use inflation::InflationModel;
use liquidity::LiquidityModel;

/// Economic and network simulation harness.
pub struct Simulation {
    pub nodes: u64,
    pub subsidy: f64,
    pub inflation: InflationModel,
    pub liquidity: LiquidityModel,
    pub bridging: BridgeModel,
    pub demand: DemandModel,
    pub backlog: f64,
}

impl Simulation {
    /// Create a new simulation with default economic models.
    pub fn new(nodes: u64) -> Self {
        Self {
            nodes,
            subsidy: 0.0,
            inflation: InflationModel::default(),
            liquidity: LiquidityModel::default(),
            bridging: BridgeModel::default(),
            demand: DemandModel::default(),
            backlog: 0.0,
        }
    }

    /// Run the simulation for the given number of steps and write a CSV dashboard.
    pub fn run(&mut self, steps: u64, out: &str) -> csv::Result<()> {
        let mut wtr = csv::Writer::from_path(out)?;
        for step in 0..steps {
            let snap = self.step(step);
            wtr.serialize(snap)?;
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
        let data = serde_json::to_vec_pretty(&g).expect("serialize governance");
        std::fs::write(path, data)
    }

    /// Advance the simulation by one step.
    pub fn step(&mut self, step: u64) -> Snapshot {
        let mut rng = rand::thread_rng();
        let inc: f64 = rng.gen_range(0.0..1.0);
        self.subsidy += inc;
        let supply = self.inflation.apply(inc);
        let liquidity = self.liquidity.update(inc);
        let bridged = self.bridging.flow(inc);
        let (consumer_demand, industrial_demand) = self.demand.project();
        let total_demand = consumer_demand + industrial_demand;
        if liquidity < total_demand {
            self.backlog += total_demand - liquidity;
        } else {
            let diff = liquidity - total_demand;
            self.backlog = (self.backlog - diff).max(0.0);
        }
        let inflation_rate = self.inflation.rate;
        let sell_coverage = if self.backlog == 0.0 {
            1.0
        } else {
            self.liquidity.token_reserve / self.backlog.max(1.0)
        };
        let readiness = 1.0 / (1.0 + self.backlog);
        Snapshot {
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
        }
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
