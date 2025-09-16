use std::fs;

use serde::Deserialize;
use tempfile::tempdir;
use the_block::governance::{
    controller, registry, GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote,
    VoteChoice,
};
use the_block::Blockchain;

#[derive(Deserialize)]
struct FeeFloorEvent {
    epoch: u64,
    proposal_id: u64,
    window: i64,
    percentile: i64,
}

fn read_fee_floor_history(base: &std::path::Path) -> Vec<FeeFloorEvent> {
    let path = base.join("governance/history/fee_floor_policy.json");
    let bytes = fs::read(path).expect("history file");
    serde_json::from_slice(&bytes).expect("fee floor history json")
}

fn submit_and_activate(
    store: &GovStore,
    params: &mut Params,
    runtime: &mut Runtime,
    key: ParamKey,
    value: i64,
    activation_epoch: u64,
) {
    let spec = registry_lookup(key);
    let proposal = Proposal {
        id: 0,
        key,
        new_value: value,
        min: spec.min,
        max: spec.max,
        proposer: "ops".into(),
        created_epoch: 0,
        vote_deadline_epoch: 0,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    };
    let pid = controller::submit_proposal(store, proposal).expect("submit");
    store
        .vote(
            pid,
            Vote {
                proposal_id: pid,
                voter: "ops".into(),
                choice: VoteChoice::Yes,
                weight: 1,
                received_at: 0,
            },
            0,
        )
        .expect("vote");
    controller::tally(store, pid, activation_epoch - 1).expect("tally");
    store
        .activate_ready(activation_epoch, runtime, params)
        .expect("activate");
}

fn registry_lookup(key: ParamKey) -> &'static the_block::governance::ParamSpec {
    registry()
        .iter()
        .find(|spec| spec.key == key)
        .expect("missing spec")
}

#[test]
fn fee_floor_policy_governance_updates_and_rollbacks() {
    let dir = tempdir().expect("tempdir");
    let store_path = dir.path().join("gov.db");
    let store = GovStore::open(&store_path);

    let mut chain = Blockchain::default();
    chain.path = dir.path().to_str().unwrap().to_string();
    let mut runtime = Runtime { bc: &mut chain };
    let mut params = Params::default();
    runtime.set_fee_floor_policy(
        params.fee_floor_window as u64,
        params.fee_floor_percentile as u64,
    );

    // Change window to 128 and activate.
    submit_and_activate(
        &store,
        &mut params,
        &mut runtime,
        ParamKey::FeeFloorWindow,
        128,
        5,
    );
    assert_eq!(params.fee_floor_window, 128);
    assert_eq!(runtime.bc.fee_floor_policy(), (128, 75));

    // Change percentile to 90 and activate at a later epoch.
    submit_and_activate(
        &store,
        &mut params,
        &mut runtime,
        ParamKey::FeeFloorPercentile,
        90,
        10,
    );
    assert_eq!(params.fee_floor_percentile, 90);
    assert_eq!(runtime.bc.fee_floor_policy(), (128, 90));

    // Ensure history contains both entries.
    let mut history = read_fee_floor_history(dir.path());
    assert_eq!(history.len(), 2);
    let latest = history.pop().unwrap();
    assert_eq!(latest.window, 128);
    assert_eq!(latest.percentile, 90);

    // Roll back the most recent activation.
    store
        .rollback_last(11, &mut runtime, &mut params)
        .expect("rollback last");
    assert_eq!(params.fee_floor_percentile, 75);
    assert_eq!(runtime.bc.fee_floor_policy(), (128, 75));

    let history_after = read_fee_floor_history(dir.path());
    assert_eq!(history_after.len(), 3);
    let reverted = history_after.last().unwrap();
    assert_eq!(reverted.window, 128);
    assert_eq!(reverted.percentile, 75);
}
