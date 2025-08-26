use tempfile::tempdir;
use the_block::governance::{
    GovStore, ParamKey, Params, Proposal, ProposalStatus, Vote, VoteChoice, ACTIVATION_DELAY,
};

#[test]
fn proposal_vote_activation_rollback() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut params = Params::default();

    // invalid proposal (out of bounds)
    let bad = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 1,
        min: 5,
        max: 10,
        proposer: "a".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
    };
    assert!(store.submit(bad).is_err());

    // valid proposal
    let good = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 10,
        min: 5,
        max: 20,
        proposer: "a".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
    };
    let pid = store.submit(good).unwrap();

    // voting after deadline rejected
    assert!(store
        .vote(
            pid,
            Vote {
                proposal_id: pid,
                voter: "v".into(),
                choice: VoteChoice::Yes,
                weight: 1,
                received_at: 0
            },
            2
        )
        .is_err());
    // vote before deadline
    store
        .vote(
            pid,
            Vote {
                proposal_id: pid,
                voter: "v".into(),
                choice: VoteChoice::Yes,
                weight: 1,
                received_at: 0,
            },
            0,
        )
        .unwrap();

    // tally before deadline -> Open
    assert_eq!(store.tally_and_queue(pid, 0).unwrap(), ProposalStatus::Open);
    // tally after deadline -> Passed
    assert_eq!(
        store.tally_and_queue(pid, 1).unwrap(),
        ProposalStatus::Passed
    );

    // no activation yet
    store.activate_ready(1, &mut params).unwrap();
    assert_ne!(params.snapshot_interval_secs, 10);
    // activation after delay
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 10);

    // rollback within window
    store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut params)
        .unwrap();
    assert_ne!(params.snapshot_interval_secs, 10);
    // second rollback fails
    assert!(store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut params)
        .is_err());

    // proposal without votes -> Rejected
    let p2 = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 15,
        min: 5,
        max: 20,
        proposer: "b".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
    };
    let pid2 = store.submit(p2).unwrap();
    assert_eq!(
        store.tally_and_queue(pid2, 1).unwrap(),
        ProposalStatus::Rejected
    );
}
