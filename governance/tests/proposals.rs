use std::collections::HashMap;

use governance::{validate_dag, ParamKey, Proposal, ProposalStatus};

fn base_proposal(id: u64) -> Proposal {
    Proposal {
        id,
        key: ParamKey::FeeFloorWindow,
        new_value: 42,
        min: 0,
        max: 1_000,
        proposer: "tester".into(),
        created_epoch: 0,
        vote_deadline_epoch: 10,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    }
}

#[test]
fn accepts_acyclic_graph() {
    let mut existing = HashMap::new();
    existing.insert(1, base_proposal(1));
    let mut p2 = base_proposal(2);
    p2.deps.push(1);
    existing.insert(2, p2);

    let mut new_prop = base_proposal(3);
    new_prop.deps.push(2);

    assert!(validate_dag(&existing, &new_prop));
}

#[test]
fn rejects_self_cycle() {
    let existing = HashMap::new();
    let mut new_prop = base_proposal(5);
    new_prop.deps.push(5);

    assert!(!validate_dag(&existing, &new_prop));
}

#[test]
fn rejects_indirect_cycle() {
    let mut existing = HashMap::new();
    let mut p1 = base_proposal(1);
    p1.deps.push(2);
    existing.insert(1, p1);
    let mut p2 = base_proposal(2);
    p2.deps.push(3);
    existing.insert(2, p2);

    let mut new_prop = base_proposal(3);
    new_prop.deps.push(1);

    assert!(!validate_dag(&existing, &new_prop));
}
