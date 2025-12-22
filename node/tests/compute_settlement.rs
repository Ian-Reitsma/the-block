#![cfg(feature = "storage-rocksdb")]

use std::fs;

use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{AuditRecord, SettleMode, Settlement};

fn teardown() {
    Settlement::shutdown();
}

#[test]
fn persists_balances_across_restart() {
    teardown();
    let dir = tempdir().expect("tempdir");
    let path = dir.path().to_str().unwrap();
    Settlement::init(path, SettleMode::Real);
    Settlement::accrue_split("provider", 100, 20);
    Settlement::shutdown();
    Settlement::init(path, SettleMode::DryRun);
    assert_eq!(Settlement::balance("provider"), 120);
    let roots = Settlement::recent_roots(1);
    assert!(!roots.is_empty());
    teardown();
}

#[test]
fn audit_records_split_and_refunds() {
    teardown();
    let dir = tempdir().expect("tempdir");
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real);
    Settlement::accrue("alice", "job", 50);
    Settlement::accrue_split("alice", 10, 5);
    Settlement::refund_split("buyer", 4, 2);
    let audit = Settlement::audit();
    assert!(contains_entry(&audit, "job", 50, None));
    assert!(contains_entry(&audit, "accrue_split", 10, Some(5)));
    assert!(contains_entry(&audit, "refund_split", 4, Some(2)));
    teardown();
}

#[test]
fn submit_anchor_appends_audit_log() {
    teardown();
    let dir = tempdir().expect("tempdir");
    let base = dir.path().to_path_buf();
    Settlement::init(base.to_str().unwrap(), SettleMode::DryRun);
    Settlement::submit_anchor(b"settle-anchor");
    Settlement::shutdown();
    let audit_path = base.join("le_audit.log");
    let contents = fs::read_to_string(audit_path).expect("audit log");
    assert!(contents.contains("compute_anchor"));
    teardown();
}

fn contains_entry(records: &[AuditRecord], memo: &str, ct: i64, it: Option<i64>) -> bool {
    records
        .iter()
        .any(|rec| rec.memo == memo && rec.delta_ct == ct && rec.delta_it == it)
}
