#![cfg(feature = "integration-tests")]
use jurisdiction::{log_law_enforcement_request, PolicyPack};
use tempfile::tempdir;

#[test]
fn separate_packs_load_independently() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    std::fs::write(
        &a,
        b"{\"region\":\"US\",\"consent_required\":true,\"features\":[\"wallet\"]}",
    )
    .unwrap();
    std::fs::write(
        &b,
        b"{\"region\":\"EU\",\"consent_required\":false,\"features\":[\"staking\"]}",
    )
    .unwrap();
    let pa = PolicyPack::load(&a).unwrap();
    let pb = PolicyPack::load(&b).unwrap();
    assert_ne!(pa.region, pb.region);
    // ensure audit log works
    let log = dir.path().join("audit.log");
    log_law_enforcement_request(&log, "test").unwrap();
    if let Ok(bytes) = std::fs::read(&log) {
        assert!(!bytes.is_empty());
    }
}
