use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[cfg(feature = "telemetry")]
use the_block::telemetry::SETTLE_APPLIED_TOTAL;

#[test]
#[serial]
fn cluster_settlement_idempotent() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let receipt = Receipt::new("job".into(), "buyer".into(), "provider".into(), 10, false);
    let key = receipt.idempotency_key;

    // Node A first run
    Settlement::init(dir1.path().to_str().unwrap(), SettleMode::Real, 0);
    Settlement::set_balance("buyer", 100);
    Settlement::set_balance("provider", 0);
    #[cfg(feature = "telemetry")]
    assert_eq!(SETTLE_APPLIED_TOTAL.get(), 0);
    Settlement::tick(1, &[receipt.clone()]);
    assert_eq!(Settlement::balance("buyer"), 90);
    #[cfg(feature = "telemetry")]
    assert_eq!(SETTLE_APPLIED_TOTAL.get(), 1);
    Settlement::shutdown();

    // Node A restart should not reapply
    Settlement::init(dir1.path().to_str().unwrap(), SettleMode::Real, 0);
    Settlement::tick(2, &[receipt.clone()]);
    assert_eq!(Settlement::balance("buyer"), 90);
    #[cfg(feature = "telemetry")]
    assert_eq!(SETTLE_APPLIED_TOTAL.get(), 1);
    Settlement::shutdown();

    // Node B first run
    Settlement::init(dir2.path().to_str().unwrap(), SettleMode::Real, 0);
    Settlement::set_balance("buyer", 100);
    Settlement::set_balance("provider", 0);
    Settlement::tick(1, &[receipt.clone()]);
    assert_eq!(Settlement::balance("buyer"), 90);
    #[cfg(feature = "telemetry")]
    assert_eq!(SETTLE_APPLIED_TOTAL.get(), 2);
    Settlement::shutdown();

    // Node B restart should also avoid reapplication
    Settlement::init(dir2.path().to_str().unwrap(), SettleMode::Real, 0);
    Settlement::tick(2, &[receipt]);
    assert_eq!(Settlement::balance("buyer"), 90);
    #[cfg(feature = "telemetry")]
    assert_eq!(SETTLE_APPLIED_TOTAL.get(), 2);
    assert!(Settlement::receipt_applied(&key));
    Settlement::shutdown();
}
