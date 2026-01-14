use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{Settlement, SettleMode};

#[test]
fn settlement_audit_smoke() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("settlement");
    let path_str = path.to_str().expect("utf8 path");

    // Initialize isolated settlement state
    Settlement::init(path_str, SettleMode::DryRun);

    // Record a simple accrual and an anchor to force audit entries
    Settlement::accrue("provider_a", "accrue_test", 10);
    Settlement::submit_anchor(b"anchor");

    let audit = Settlement::audit();
    assert!(
        audit.iter().any(|rec| rec.memo == "accrue_test"),
        "audit log should contain accrue entry"
    );

    Settlement::shutdown();
}
