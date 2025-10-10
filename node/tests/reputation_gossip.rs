#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::compute_market::scheduler::ReputationStore;

#[test]
fn reputation_gossip_converges() {
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let mut a = ReputationStore::load(dir_a.path().join("rep.json"));
    let mut b = ReputationStore::load(dir_b.path().join("rep.json"));

    a.adjust("prov1", 50);
    let snap = a.snapshot();
    assert_eq!(snap.len(), 1);
    let g = &snap[0];
    assert!(b.merge(&g.provider_id, g.reputation_score, g.epoch));
    assert_eq!(b.get("prov1"), 50);
    // stale update ignored
    assert!(!b.merge(&g.provider_id, g.reputation_score - 10, g.epoch - 1));
    assert_eq!(b.get("prov1"), 50);
}
