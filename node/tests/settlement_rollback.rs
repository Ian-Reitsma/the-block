use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
#[serial]
fn rollback_clears_tampered_receipts() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 5);
    let r = Receipt::new("job".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(1, &[r.clone()]);
    let pending = dir.path().join("receipts/pending/1");
    let mut list: Vec<Receipt> = bincode::deserialize(&std::fs::read(&pending).unwrap()).unwrap();
    list[0].quote_price = 5;
    let bytes = bincode::serialize(&list).unwrap();
    std::fs::write(&pending, bytes).unwrap();
    let res = Settlement::audit();
    assert_eq!(res[0].invalid, 1);
    std::fs::remove_file(pending).unwrap();
    let res = Settlement::audit();
    assert!(res.is_empty());
    Settlement::shutdown();
}
