use tb_sim::Simulation;

#[test]
fn exports_governance_template() {
    let mut sim = Simulation::new(1);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    sim.step(0);
    sim.export_governance(tmp.path()).unwrap();
    let data = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(data.contains("total_credits"));
}
