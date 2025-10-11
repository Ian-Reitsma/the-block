use tb_sim::Simulation;

#[test]
fn writes_kpis_to_csv() {
    let mut sim = Simulation::new(1);
    let tmp = sys::tempfile::NamedTempFile::new().unwrap();
    sim.run(2, tmp.path().to_str().unwrap()).unwrap();
    let data = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(data.contains("inflation_rate"));
    assert!(data.contains("sell_coverage"));
    assert!(data.contains("readiness"));
}
