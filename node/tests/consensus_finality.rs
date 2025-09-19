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

    // Simulate fault then rollback
    engine.rollback();
    assert!(!engine.vote("v1", "B"));
    assert!(engine.vote("v2", "B"));
    assert_eq!(engine.gadget.finalized(), Some("B"));
}
