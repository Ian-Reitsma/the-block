use std::collections::HashMap;
use the_block::consensus::{engine::ConsensusEngine, unl::Unl};

#[test]
fn adversarial_rollback_restores_consistency() {
    let mut unl = Unl::default();
    unl.add_validator("v1".into(), 10);
    unl.add_validator("v2".into(), 10);
    unl.add_validator("v3".into(), 10);

    // finalize block A
    let mut engine = ConsensusEngine::new(unl.clone());
    assert!(!engine.vote("v1", "A"));
    assert!(engine.vote("v2", "A"));
    assert_eq!(engine.gadget.finalized(), Some("A"));
    let mut ledger: Vec<&'static str> = vec!["A"];
    let mut balances: HashMap<&'static str, i32> = HashMap::new();
    balances.insert("alice", 1);

    // finalize conflicting block B1 on new height
    let mut engine = ConsensusEngine::new(unl.clone());
    assert!(!engine.vote("v1", "B1"));
    assert!(engine.vote("v2", "B1"));
    assert_eq!(engine.gadget.finalized(), Some("B1"));
    ledger.push("B1");
    *balances.get_mut("alice").unwrap() += 1;

    // B1 found faulty; rollback and finalize B2 instead
    engine.rollback();
    ledger.pop();
    *balances.get_mut("alice").unwrap() -= 1;
    assert!(!engine.vote("v2", "B2"));
    assert!(engine.vote("v3", "B2"));
    assert_eq!(engine.gadget.finalized(), Some("B2"));
    ledger.push("B2");
    *balances.get_mut("alice").unwrap() += 2;

    assert_eq!(ledger, vec!["A", "B2"]);
    assert_eq!(*balances.get("alice").unwrap(), 3);
}
