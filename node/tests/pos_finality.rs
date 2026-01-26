#![cfg(feature = "integration-tests")]
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

#[test]
fn partitions_resume_finality_after_equivocation() {
    let mut pos = PosState::default();
    for id in ["v1", "v2", "v3", "v4"] {
        pos.register(id.into());
        pos.bond(id, "validator", 25);
    }
    let mut engine = ConsensusEngine::new(pos.unl());

    // Two disjoint partitions each hold 50% stake and neither finalizes alone.
    assert!(!engine.vote("v1", "A"));
    assert!(!engine.vote("v2", "A"));
    assert!(!engine.vote("v3", "B"));
    assert!(!engine.vote("v4", "B"));
    assert_eq!(engine.gadget.finalized(), None);

    // Partition heals, conflicting votes are discarded but finality resumes with the honest subset.
    let _ = engine.vote("v3", "A");
    let _ = engine.vote("v4", "A");
    assert_eq!(engine.gadget.finalized(), Some("A"));

    let snapshot = engine.snapshot();
    assert!(snapshot.equivocations.contains("v3"));
    assert!(snapshot.equivocations.contains("v4"));
    assert_eq!(
        snapshot.effective_total_stake,
        snapshot.total_stake - snapshot.equivocated_stake
    );
    assert!(snapshot.finality_threshold <= snapshot.effective_total_stake);
    assert_eq!(snapshot.votes.len(), 2);
}

#[test]
fn equivocation_stake_is_excluded_from_thresholds() {
    let mut pos = PosState::default();
    pos.register("v1".into());
    pos.register("v2".into());
    pos.register("v3".into());
    pos.bond("v1", "validator", 45);
    pos.bond("v2", "validator", 30);
    pos.bond("v3", "validator", 25);

    let mut engine = ConsensusEngine::new(pos.unl());
    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));

    engine.rollback();
    assert!(!engine.vote("v1", "A"));
    assert!(!engine.vote("v1", "B")); // equivocation removes 45 stake.
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
    assert!(snapshot.equivocations.contains("v1"));
}
