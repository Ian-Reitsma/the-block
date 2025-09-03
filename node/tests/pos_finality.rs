use the_block::consensus::{engine::ConsensusEngine, pos::PosState};

#[test]
fn stake_weights_drive_finality() {
    let mut pos = PosState::default();
    pos.register("v1".into());
    pos.register("v2".into());
    pos.register("v3".into());
    pos.bond("v1", "validator", 10);
    pos.bond("v2", "validator", 10);
    pos.bond("v3", "validator", 10);
    // Initial votes finalize with two validators.
    let mut engine = ConsensusEngine::new(pos.unl());
    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));

    // Slash v2 and ensure new weights are respected.
    pos.slash("v2", "validator", 10);
    let mut engine = ConsensusEngine::new(pos.unl());
    assert!(!engine.vote("v1", "B"));
    assert!(engine.vote("v3", "B"));
    assert_eq!(engine.gadget.finalized(), Some("B"));

    // Unbond v3 entirely, leaving only v1 with stake.
    pos.unbond("v3", "validator", 10);
    let mut engine = ConsensusEngine::new(pos.unl());
    assert!(engine.vote("v1", "C"));
    assert_eq!(engine.gadget.finalized(), Some("C"));
}
