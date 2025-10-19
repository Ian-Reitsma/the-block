use std::fs;
use std::time::Duration;

use foundation_serialization::json::Value;
use governance::{
    codec::json_from_bytes, controller, registry, GovStore, ParamKey, Params, Proposal,
    ProposalStatus, Runtime, RuntimeAdapter, Vote, VoteChoice,
};
use sys::tempfile::tempdir;

#[allow(dead_code)]
struct FeeFloorEvent {
    epoch: u64,
    proposal_id: u64,
    window: i64,
    percentile: i64,
}

impl FeeFloorEvent {
    fn from_json(value: &Value) -> Option<Self> {
        let obj = value.as_object()?;
        Some(Self {
            epoch: obj.get("epoch")?.as_u64()?,
            proposal_id: obj.get("proposal_id")?.as_u64()?,
            window: obj.get("window")?.as_i64()?,
            percentile: obj.get("percentile")?.as_i64()?,
        })
    }
}

fn read_fee_floor_history(base: &std::path::Path) -> Vec<FeeFloorEvent> {
    let path = base.join("governance/history/fee_floor_policy.json");
    let bytes = fs::read(path).expect("history file");
    let value = json_from_bytes(&bytes).expect("fee floor history json");
    value
        .as_array()
        .expect("fee floor history array")
        .iter()
        .map(|entry| FeeFloorEvent::from_json(entry).expect("fee floor history entry"))
        .collect()
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
    let vote_deadline = activation_epoch.saturating_sub(governance::ACTIVATION_DELAY);
    let proposal = Proposal {
        id: 0,
        key,
        new_value: value,
        min: spec.min,
        max: spec.max,
        proposer: "ops".into(),
        created_epoch: 0,
        vote_deadline_epoch: vote_deadline,
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
    controller::tally(store, pid, vote_deadline).expect("tally");
    store
        .activate_ready(activation_epoch, runtime, params)
        .expect("activate");
}

fn registry_lookup(key: ParamKey) -> &'static governance::ParamSpec {
    registry()
        .iter()
        .find(|spec| spec.key == key)
        .expect("missing spec")
}

struct MockRuntime {
    window: u64,
    percentile: u64,
    consumer_comfort: u64,
    snapshot_secs: u64,
}

impl MockRuntime {
    fn new(params: &Params) -> Self {
        Self {
            window: params.fee_floor_window as u64,
            percentile: params.fee_floor_percentile as u64,
            consumer_comfort: params.consumer_fee_comfort_p90_microunits as u64,
            snapshot_secs: params.snapshot_interval_secs as u64,
        }
    }
}

impl RuntimeAdapter for MockRuntime {
    fn set_fee_floor_policy(&mut self, window: u64, percentile: u64) {
        self.window = window;
        self.percentile = percentile;
    }

    fn set_snapshot_interval(&mut self, d: Duration) {
        self.snapshot_secs = d.as_secs();
    }

    fn set_consumer_p90_comfort(&mut self, value: u64) {
        self.consumer_comfort = value;
    }

    fn fee_floor_policy(&mut self) -> Option<(u64, u64)> {
        Some((self.window, self.percentile))
    }
}

#[test]
fn fee_floor_policy_governance_updates_and_rollbacks() {
    let dir = tempdir().expect("tempdir");
    let store_path = dir.path().join("gov.db");
    let store = GovStore::open(&store_path);

    let mut params = Params::default();
    let mut adapter = MockRuntime::new(&params);
    let mut runtime = Runtime::new(&mut adapter);
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
    drop(runtime);
    assert_eq!((adapter.window, adapter.percentile), (128, 75));
    let mut runtime = Runtime::new(&mut adapter);

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
    drop(runtime);
    assert_eq!((adapter.window, adapter.percentile), (128, 90));
    let mut runtime = Runtime::new(&mut adapter);

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
    drop(runtime);
    assert_eq!((adapter.window, adapter.percentile), (128, 75));

    let history_after = read_fee_floor_history(dir.path());
    assert_eq!(history_after.len(), 3);
    let reverted = history_after.last().unwrap();
    assert_eq!(reverted.window, 128);
    assert_eq!(reverted.percentile, 75);
}
