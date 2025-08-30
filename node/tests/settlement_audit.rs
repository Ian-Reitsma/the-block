use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{AuditSummary, SettleMode, Settlement};

#[test]
fn audit_detects_tampering() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 5);
    let r = Receipt::new("job".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(1, &[r.clone()]);
    // tamper with receipt file
    let pending = dir.path().join("receipts/pending/1");
    let mut list: Vec<Receipt> = bincode::deserialize(&std::fs::read(&pending).unwrap()).unwrap();
    list[0].quote_price = 5; // change field so idempotency key mismatch
    let bytes = bincode::serialize(&list).unwrap();
    std::fs::write(&pending, bytes).unwrap();
    let res = Settlement::audit();
    assert_eq!(res.len(), 1);
    let AuditSummary {
        receipts, invalid, ..
    } = res[0];
    assert_eq!(receipts, 1);
    assert_eq!(invalid, 1);
}
