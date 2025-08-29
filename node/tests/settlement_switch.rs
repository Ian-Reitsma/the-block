use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn arm_and_activate() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 100);
    Settlement::set_balance("buyer", 100);
    Settlement::set_balance("prov", 0);
    let r1 = Receipt::new("j1".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::arm(5, 10);
    for h in 11..15 {
        Settlement::tick(h, &[r1.clone()]);
    }
    assert_eq!(Settlement::balance("buyer"), 100);
    let r2 = Receipt::new("j2".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(15, &[r2.clone()]);
    assert_eq!(Settlement::balance("buyer"), 90);
    assert_eq!(Settlement::balance("prov"), 10);
    assert!(Settlement::receipt_applied(&r2.idempotency_key));
}

#[test]
#[serial]
fn insufficient_funds_flips() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 100);
    Settlement::set_balance("buyer", 5);
    Settlement::set_balance("prov", 0);
    let r = Receipt::new("j1".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(1, &[r]);
    assert_eq!(Settlement::mode(), SettleMode::DryRun);
    assert_eq!(Settlement::balance("buyer"), 5);
    assert_eq!(Settlement::balance("prov"), 0);
}

#[test]
#[serial]
fn cancel_arm_before_activation() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 100);
    Settlement::arm(5, 10);
    Settlement::cancel_arm();
    let r = Receipt::new("j1".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::set_balance("buyer", 100);
    Settlement::set_balance("prov", 0);
    Settlement::tick(20, &[r]);
    assert_eq!(Settlement::balance("prov"), 0);
}

#[test]
#[serial]
fn idempotent_replay() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 100);
    Settlement::set_balance("buyer", 50);
    Settlement::set_balance("prov", 0);
    let r = Receipt::new("j1".into(), "buyer".into(), "prov".into(), 20, false);
    let key = r.idempotency_key;
    Settlement::tick(1, &[r.clone()]);
    Settlement::tick(2, &[r]);
    assert_eq!(Settlement::balance("buyer"), 30);
    assert_eq!(Settlement::balance("prov"), 20);
    assert!(Settlement::receipt_applied(&key));
}
