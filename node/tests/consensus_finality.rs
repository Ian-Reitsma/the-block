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
fn equivocation_stake_is_excluded_from_tally() {
    let mut unl = Unl::default();
    unl.add_validator("v1".into(), 45);
    unl.add_validator("v2".into(), 30);
    unl.add_validator("v3".into(), 25);
    let mut engine = ConsensusEngine::new(unl);

    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));

    engine.rollback();
    assert!(!engine.vote("v1", "A"));
    assert!(!engine.vote("v1", "B"));
    assert_eq!(engine.gadget.finalized(), None);
    let snapshot = engine.snapshot();
    assert!(snapshot.equivocations.contains("v1"));
    assert_eq!(snapshot.equivocated_stake, 45);
    assert_eq!(
        snapshot.effective_total_stake,
        snapshot.total_stake - snapshot.equivocated_stake
    );
    assert_eq!(snapshot.finality_threshold, 37);

    assert!(!engine.vote("v2", "B"));
    assert!(engine.vote("v3", "B"));
    assert_eq!(engine.gadget.finalized(), Some("B"));
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.finalized.as_deref(), Some("B"));
}
