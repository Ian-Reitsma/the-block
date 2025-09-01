use tb_sim::Simulation;

fn main() {
    let mut sim = Simulation::new(100);
    sim.inflation.rate = 0.02;
    sim.demand.consumer_growth = 0.05;
    sim.demand.industrial_growth = 0.04;
    if let Err(e) = sim.run(50, "/tmp/gov_tuning.csv") {
        eprintln!("simulation failed: {e}");
    }
}
