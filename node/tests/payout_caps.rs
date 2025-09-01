use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn cap_enforced() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    Settlement::set_balance("buyer", 1_000);
    Settlement::set_daily_payout_cap(50);
    let r1 = Receipt::new("j1".into(), "buyer".into(), "prov".into(), 40, false);
    Settlement::tick(1, &[r1]);
    assert_eq!(Settlement::balance("prov"), 40);
    let r2 = Receipt::new("j2".into(), "buyer".into(), "prov".into(), 40, false);
    Settlement::tick(2, &[r2]);
    assert_eq!(Settlement::balance("prov"), 50);
    Settlement::shutdown();
}
