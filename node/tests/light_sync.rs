#![cfg(feature = "integration-tests")]

use sys::tempfile::tempdir;
use the_block::light_client::proof_tracker::ProofTracker;

#[test]
fn proof_tracker_snapshot_starts_empty() {
    let dir = tempdir().unwrap();
    let tracker = ProofTracker::open(dir.path());
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.pending_total, 0);
    assert!(snapshot.relayers.is_empty());
}
