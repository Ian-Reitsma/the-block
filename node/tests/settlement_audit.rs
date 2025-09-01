use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::receipt::Receipt;
use the_block::compute_market::settlement::{AuditSummary, SettleMode, Settlement};

#[test]
#[serial]
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
    #[cfg(feature = "telemetry")]
    assert_eq!(the_block::telemetry::SETTLE_AUDIT_MISMATCH_TOTAL.get(), 1);
    Settlement::shutdown();
}

#[test]
#[serial]
fn audit_job_runs() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_SETTLE_AUDIT_INTERVAL_MS", "10");
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 5);
    let r = Receipt::new("job".into(), "buyer".into(), "prov".into(), 10, false);
    Settlement::tick(1, &[r.clone()]);
    let pending = dir.path().join("receipts/pending/1");
    let mut list: Vec<Receipt> = bincode::deserialize(&std::fs::read(&pending).unwrap()).unwrap();
    list[0].quote_price = 5;
    let bytes = bincode::serialize(&list).unwrap();
    std::fs::write(&pending, bytes).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let report = dir.path().join("receipts/audit_latest.json");
    assert!(report.exists(), "audit report missing");
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::SETTLE_AUDIT_MISMATCH_TOTAL.get() > 0);
    Settlement::shutdown();
    std::env::remove_var("TB_SETTLE_AUDIT_INTERVAL_MS");
}
