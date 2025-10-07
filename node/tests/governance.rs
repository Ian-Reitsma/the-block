#![cfg(feature = "integration-tests")]
use governance_spec::{
    encode_runtime_backend_policy, encode_storage_engine_policy, encode_transport_provider_policy,
};
use serde_json::json;
use tempfile::tempdir;
use the_block::governance::{
    GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
    ACTIVATION_DELAY,
};
use the_block::Blockchain;

#[test]
fn proposal_vote_activation_rollback() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut params = Params::default();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    let mut rt = Runtime { bc: &mut bc };

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
        deps: Vec::new(),
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
        deps: Vec::new(),
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
    store.activate_ready(1, &mut rt, &mut params).unwrap();
    assert_ne!(params.snapshot_interval_secs, 10);
    // activation after delay
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 10);

    // rollback within window
    store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut rt, &mut params)
        .unwrap();
    assert_ne!(params.snapshot_interval_secs, 10);
    // second rollback fails
    assert!(store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut rt, &mut params)
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
        deps: Vec::new(),
    };
    let pid2 = store.submit(p2).unwrap();
    assert_eq!(
        store.tally_and_queue(pid2, 1).unwrap(),
        ProposalStatus::Rejected
    );
}

#[test]
fn gov_params_includes_dependency_policy() {
    let runtime_mask = encode_runtime_backend_policy(["inhouse", "stub"]).unwrap();
    let transport_mask = encode_transport_provider_policy(["quinn"]).unwrap();
    let storage_mask = encode_storage_engine_policy(["rocksdb-compat", "inhouse"]).unwrap();

    let mut params = Params::default();
    params.runtime_backend_policy = runtime_mask;
    params.transport_provider_policy = transport_mask;
    params.storage_engine_policy = storage_mask;

    let response = the_block::rpc::governance::gov_params(&params, 42).unwrap();

    assert_eq!(response["runtime_backend_mask"], json!(runtime_mask));
    assert_eq!(
        response["runtime_backend_policy"],
        json!(["inhouse", "stub"])
    );
    assert_eq!(response["transport_provider_mask"], json!(transport_mask));
    assert_eq!(response["transport_provider_policy"], json!(["quinn"]));
    assert_eq!(response["storage_engine_mask"], json!(storage_mask));
    assert_eq!(
        response["storage_engine_policy"],
        json!(["rocksdb-compat", "inhouse"])
    );
}
