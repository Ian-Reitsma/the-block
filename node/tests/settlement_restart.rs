use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn receipts_not_double_applied_across_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();

    Settlement::init(path, SettleMode::Real, 0, 0.0, 0);
    Settlement::set_balance("buyer", 100);
    Settlement::set_balance("provider", 0);

    let receipt = Receipt::new("job".into(), "buyer".into(), "provider".into(), 10, false);
    let key = receipt.idempotency_key;

    Settlement::tick(1, &[receipt.clone()]);
    assert_eq!(Settlement::balance("buyer"), 90);
    assert_eq!(Settlement::balance("provider"), 10);
    assert!(Settlement::receipt_applied(&key));

    Settlement::shutdown();

    Settlement::init(path, SettleMode::Real, 0, 0.0, 0);
    Settlement::tick(2, &[receipt]);
    assert_eq!(Settlement::balance("buyer"), 90);
    assert_eq!(Settlement::balance("provider"), 10);
    assert!(Settlement::receipt_applied(&key));
    Settlement::shutdown();
}
