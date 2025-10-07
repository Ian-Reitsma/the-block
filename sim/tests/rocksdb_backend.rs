use tb_sim::{Backend, Simulation};

#[test]
#[should_panic(expected = "legacy RocksDB backend has been removed")]
fn legacy_backend_is_disabled() {
    let _ = Simulation::with_backend(1, Backend::LegacyRocksDb);
}
