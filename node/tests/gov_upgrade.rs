#![cfg(feature = "integration-tests")]
use sim::Simulation;
use std::fs;

#[test]
fn governance_upgrade_export() {
    std::env::set_var("TB_SIM_SEED", "42");
    let sim = Simulation::new(1);
    let path = std::env::temp_dir().join("gov.json");
    sim.export_governance(&path).expect("export governance");
    let data = fs::read_to_string(path).expect("read export");
    assert!(data.contains("total_subsidy"));
}
