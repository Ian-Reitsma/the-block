use light_client::proof_tracker::ProofTracker;

#[test]
fn prevent_double_claim() {
    let mut t = ProofTracker::default();
    t.record(vec![1], 5);
    assert_eq!(t.claim_all(), 5);
    // second claim without new proofs yields zero
    assert_eq!(t.claim_all(), 0);
}
