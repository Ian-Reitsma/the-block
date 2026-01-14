use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};

#[test]
fn settlement_audit_smoke() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("settlement");
    let path_str = path.to_str().expect("utf8 path");

    // Initialize isolated settlement state
    Settlement::init(path_str, SettleMode::DryRun);

    // Record a simple accrual and an anchor to force audit entries
    Settlement::accrue("provider_a", "accrue_test", 10);
    Settlement::accrue("treasury", "treasury_inflow", 5);
    Settlement::submit_anchor(b"anchor");

    let audit = Settlement::audit();
    let accrue_entry = audit
        .iter()
        .find(|rec| rec.memo == "accrue_test")
        .expect("audit log should contain accrue entry");
    assert_eq!(accrue_entry.delta, 10);
    assert_eq!(accrue_entry.balance, 10);
    assert!(
        audit.iter().any(|rec| rec.anchor.is_some()),
        "audit log should contain anchor entry"
    );

    let treasury_entry = audit
        .iter()
        .find(|rec| rec.entity == "treasury")
        .expect("treasury delta should be recorded");
    assert_eq!(treasury_entry.delta, 5);
    assert_eq!(treasury_entry.balance, 5);

    Settlement::shutdown();
}
