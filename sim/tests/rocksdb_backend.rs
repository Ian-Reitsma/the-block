use tb_sim::{Backend, Simulation};

#[test]
fn stores_snapshots_in_rocksdb() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SIM_DB_PATH", dir.path());
    let mut sim = Simulation::with_backend(1, Backend::RocksDb);
    sim.step(1);
    drop(sim);
    let db = rocksdb::DB::open_default(dir.path()).unwrap();
    let snap = db.get(1u64.to_be_bytes()).unwrap().unwrap();
    assert!(!snap.is_empty());
    std::env::remove_var("SIM_DB_PATH");
}
