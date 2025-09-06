use tb_sim::Simulation;

fn main() {
    let mut sim = Simulation::new(100);
    sim.inflation.rate = 0.02;
    sim.demand.consumer_growth = 0.05;
    sim.demand.industrial_growth = 0.04;
    for i in 0..3 {
        sim.demand.industrial_growth += 0.02;
        if let Err(e) = sim.run(20, &format!("/tmp/gov_tuning_{i}.csv")) {
            eprintln!("simulation failed: {e}");
        }
    }
}
