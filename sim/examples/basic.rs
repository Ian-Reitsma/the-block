use tb_sim::Simulation;

fn main() {
    let mut sim = Simulation::new(10);
    sim.run(5, "/tmp/out.csv");
    println!("done");
}
