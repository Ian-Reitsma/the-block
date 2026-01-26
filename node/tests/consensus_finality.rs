#![cfg(feature = "integration-tests")]
use the_block::consensus::{engine::ConsensusEngine, unl::Unl};

#[test]
fn finality_and_rollback() {
    let mut unl = Unl::default();
    unl.add_validator("v1".into(), 10);
    unl.add_validator("v2".into(), 10);
    unl.add_validator("v3".into(), 10);
    let mut engine = ConsensusEngine::new(unl);
    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.equivocations.len(), 0);

    // Simulate fault then rollback
    engine.rollback();
    assert!(!engine.vote("v1", "B"));
    assert!(engine.vote("v2", "B"));
    assert_eq!(engine.gadget.finalized(), Some("B"));
}

#[test]
fn equivocation_removes_stake_and_blocks_conflicting_finality() {
    let mut unl = Unl::default();
    unl.add_validator("v1".into(), 40);
    unl.add_validator("v2".into(), 40);
    unl.add_validator("v3".into(), 20);
    let mut engine = ConsensusEngine::new(unl);

    // Honest votes for A from v1 and v2 would normally finalize.
    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));

    // Roll back and have v1 equivocate; stake should be discarded.
    engine.rollback();
    assert!(!engine.vote("v1", "A"));
    assert!(!engine.vote("v1", "B")); // equivocation, ignored
    assert!(!engine.vote("v2", "B")); // remaining honest stake 40/100 < 2/3
    assert_eq!(engine.gadget.finalized(), None);
    let snap = engine.snapshot();
    assert!(snap.equivocations.contains("v1"));
    assert_eq!(snap.finalized, None);
}
