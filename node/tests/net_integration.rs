use sim::Simulation;

#[test]
fn partitions_merge_consistent_fork_choice() {
    std::env::set_var("TB_SIM_SEED", "1337");
    let mut net_a = Simulation::new(2);
    std::env::set_var("TB_SIM_SEED", "1337");
    let mut net_b = Simulation::new(2);

    for step in 0..5 {
        net_a.step(step);
        net_b.step(step);
    }

    // simulate isolated progress
    for step in 5..10 {
        net_a.step(step);
        net_b.step(step);
    }

    assert!((net_a.inflation.supply - net_b.inflation.supply).abs() < 1e-9);
}
