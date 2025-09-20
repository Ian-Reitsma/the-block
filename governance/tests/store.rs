use governance::{
    controller, GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, RuntimeAdapter,
    Vote, VoteChoice, ACTIVATION_DELAY,
};
use tempfile::tempdir;

struct NoopAdapter;

impl RuntimeAdapter for NoopAdapter {}

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
