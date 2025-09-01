use credits::Source;
use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn meter_reports_source_balances() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    Settlement::accrue("prov", "e1", Source::Civic, 50, u64::MAX);
    let map = Settlement::meter("prov");
    assert_eq!(map.get(&Source::Civic).unwrap().0, 50);
    Settlement::shutdown();
}
