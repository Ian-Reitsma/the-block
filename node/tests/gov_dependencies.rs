#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::governance::{
    controller, GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
};

#[test]
fn dependency_blocks_vote() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let p1 = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 10,
        min: 1,
        max: 100,
        proposer: "a".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: vec![],
    };
    let id1 = controller::submit_proposal(&store, p1).unwrap();
    let mut bc = the_block::Blockchain::default();
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    store
        .vote(
            id1,
            Vote {
                proposal_id: id1,
                voter: "bootstrap".into(),
                choice: VoteChoice::Yes,
                weight: 1,
                received_at: 0,
            },
            0,
        )
        .unwrap();
    controller::tally(&store, id1, 2).unwrap();
    controller::activate_ready(&store, 5, &mut rt, &mut params).unwrap();

    let p2 = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 20,
        min: 1,
        max: 100,
        proposer: "b".into(),
        created_epoch: 0,
        vote_deadline_epoch: 2,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: vec![id1],
    };
    let id2 = controller::submit_proposal(&store, p2).unwrap();
    // voting should succeed since dependency activated
    let res = the_block::rpc::governance::vote_proposal(&store, "v".into(), id2, "yes", 0);
    assert!(res.is_ok());
}

#[test]
fn cycle_rejected() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let p1 = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 10,
        min: 1,
        max: 100,
        proposer: "a".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: vec![1],
    };
    assert!(controller::submit_proposal(&store, p1).is_err());
}
