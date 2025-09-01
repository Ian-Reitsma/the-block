use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn dispute_prevents_finalization() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 1);
    Settlement::set_balance("buyer", 100);
    let r = Receipt::new("job".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(1, &[r.clone()]);
    assert_eq!(Settlement::balance(&r.provider), 0);
    assert!(Settlement::dispute(1, r.idempotency_key));
    Settlement::tick(2, &[]);
    assert_eq!(Settlement::balance(&r.provider), 0);
    let r2 = Receipt::new("job2".into(), "buyer".into(), "prov".into(), 5, false);
    Settlement::tick(3, &[r2.clone()]);
    Settlement::tick(4, &[]);
    assert_eq!(Settlement::balance(&r2.provider), r2.quote_price);
    Settlement::shutdown();
}
