use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn tardy_provider_penalized() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    Settlement::set_balance("p1", 100);
    Settlement::penalize_sla("p1", 40).expect("penalize");
    assert_eq!(Settlement::balance("p1"), 60);
    Settlement::shutdown();
}
