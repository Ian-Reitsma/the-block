use governance::{
    controller, decode_runtime_backend_policy, decode_storage_engine_policy,
    decode_transport_provider_policy, encode_runtime_backend_policy, encode_storage_engine_policy,
    encode_transport_provider_policy, GovStore, ParamKey, Params, Proposal, ProposalStatus,
    Runtime, RuntimeAdapter, Vote, VoteChoice, ACTIVATION_DELAY, RUNTIME_BACKEND_OPTIONS,
    STORAGE_ENGINE_OPTIONS, TRANSPORT_PROVIDER_OPTIONS,
};
use tempfile::tempdir;

struct NoopAdapter;

impl RuntimeAdapter for NoopAdapter {}

#[derive(Default)]
struct RecordingAdapter {
    runtime: Vec<String>,
    transport: Vec<String>,
    storage: Vec<String>,
}

impl RuntimeAdapter for RecordingAdapter {
    fn set_runtime_backend_policy(&mut self, allowed: &[String]) {
        self.runtime = allowed.to_vec();
    }

    fn set_transport_provider_policy(&mut self, allowed: &[String]) {
        self.transport = allowed.to_vec();
    }

    fn set_storage_engine_policy(&mut self, allowed: &[String]) {
        self.storage = allowed.to_vec();
    }
}

#[test]
fn proposal_activation_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = GovStore::open(&path);

    let proposal = Proposal {
        id: 0,
        key: ParamKey::FeeFloorWindow,
        new_value: 512,
        min: 1,
        max: 2048,
        proposer: "cli".into(),
        created_epoch: 0,
        vote_deadline_epoch: 2,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    };
    let proposal_id = controller::submit_proposal(&store, proposal.clone()).unwrap();

    let vote = Vote {
        proposal_id,
        voter: "tester".into(),
        choice: VoteChoice::Yes,
        weight: 1,
        received_at: 0,
    };
    store.vote(proposal_id, vote, 1).unwrap();
    store.tally_and_queue(proposal_id, 3).unwrap();

    let mut params = Params::default();
    let mut adapter = NoopAdapter;
    let mut runtime = Runtime::new(&mut adapter);
    let activation_epoch = 3 + ACTIVATION_DELAY;
    store
        .activate_ready(activation_epoch, &mut runtime, &mut params)
        .unwrap();

    assert_eq!(params.fee_floor_window, 512);
    let last = store.last_activation_record().unwrap().unwrap();
    assert_eq!(last.proposal_id, proposal_id);
    assert_eq!(last.new_value, 512);

    drop(store);
    let reopened = GovStore::open(&path);
    let persisted = reopened.last_activation_record().unwrap().unwrap();
    assert_eq!(persisted.new_value, 512);
}

#[test]
fn dependency_policy_activation_records_history() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = GovStore::open(&path);

    let runtime_mask = encode_runtime_backend_policy(["inhouse", "stub"]).unwrap();
    let transport_mask = encode_transport_provider_policy(["quinn"]).unwrap();
    let storage_mask = encode_storage_engine_policy(["rocksdb-compat", "inhouse"]).unwrap();

    let specs = [
        (
            ParamKey::RuntimeBackend,
            runtime_mask,
            (1, (1i64 << RUNTIME_BACKEND_OPTIONS.len()) - 1),
        ),
        (
            ParamKey::TransportProvider,
            transport_mask,
            (1, (1i64 << TRANSPORT_PROVIDER_OPTIONS.len()) - 1),
        ),
        (
            ParamKey::StorageEnginePolicy,
            storage_mask,
            (1, (1i64 << STORAGE_ENGINE_OPTIONS.len()) - 1),
        ),
    ];

    for (idx, (key, value, (min, max))) in specs.into_iter().enumerate() {
        let proposal = Proposal {
            id: 0,
            key,
            new_value: value,
            min,
            max,
            proposer: format!("p{idx}"),
            created_epoch: 0,
            vote_deadline_epoch: 2,
            activation_epoch: None,
            status: ProposalStatus::Open,
            deps: Vec::new(),
        };
        let proposal_id = controller::submit_proposal(&store, proposal).unwrap();
        let vote = Vote {
            proposal_id,
            voter: "tester".into(),
            choice: VoteChoice::Yes,
            weight: 1,
            received_at: 0,
        };
        store.vote(proposal_id, vote, 0).unwrap();
        store.tally_and_queue(proposal_id, 3).unwrap();
    }

    let mut params = Params::default();
    let mut adapter = RecordingAdapter::default();
    let mut runtime = Runtime::new(&mut adapter);
    let activation_epoch = 3 + ACTIVATION_DELAY;
    store
        .activate_ready(activation_epoch, &mut runtime, &mut params)
        .unwrap();

    assert_eq!(params.runtime_backend_policy, runtime_mask);
    assert_eq!(params.transport_provider_policy, transport_mask);
    assert_eq!(params.storage_engine_policy, storage_mask);

    assert_eq!(adapter.runtime, decode_runtime_backend_policy(runtime_mask));
    assert_eq!(
        adapter.transport,
        decode_transport_provider_policy(transport_mask)
    );
    assert_eq!(adapter.storage, decode_storage_engine_policy(storage_mask));

    let history = store.dependency_policy_history().unwrap();
    assert_eq!(history.len(), 3);
    assert!(history
        .iter()
        .any(|rec| rec.kind == "runtime_backend" && rec.allowed == adapter.runtime));
    assert!(history
        .iter()
        .any(|rec| rec.kind == "transport_provider" && rec.allowed == adapter.transport));
    assert!(history
        .iter()
        .any(|rec| rec.kind == "storage_engine" && rec.allowed == adapter.storage));
}

#[test]
fn dependency_policy_rejects_invalid_mask() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("store.db");
    let store = GovStore::open(&path);

    let invalid_runtime = Proposal {
        id: 0,
        key: ParamKey::RuntimeBackend,
        new_value: 0,
        min: 0,
        max: (1 << RUNTIME_BACKEND_OPTIONS.len()) - 1,
        proposer: "cli".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    };
    let proposal_id = controller::submit_proposal(&store, invalid_runtime).unwrap();
    let vote = Vote {
        proposal_id,
        voter: "tester".into(),
        choice: VoteChoice::Yes,
        weight: 1,
        received_at: 0,
    };
    store.vote(proposal_id, vote, 0).unwrap();
    store.tally_and_queue(proposal_id, 2).unwrap();

    let mut params = Params::default();
    let mut adapter = NoopAdapter;
    let mut runtime = Runtime::new(&mut adapter);
    let activation_epoch = 2 + ACTIVATION_DELAY;
    let err = store
        .activate_ready(activation_epoch, &mut runtime, &mut params)
        .unwrap_err();
    assert!(format!("{err}").contains("apply"));

    let history = store.dependency_policy_history().unwrap();
    assert!(history.is_empty());
}
