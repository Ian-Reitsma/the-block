use tb_sim::Simulation;

fn main() {
    let mut sim = Simulation::new(10);
    sim.start_partition(3);
    if let Err(e) = sim.run(5, "/tmp/out.csv") {
        eprintln!("simulation failed: {e}");
    }
    println!("done");
}
