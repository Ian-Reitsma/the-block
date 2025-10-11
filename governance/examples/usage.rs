use governance::{
    controller, registry, GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime,
    RuntimeAdapter, Vote, VoteChoice, ACTIVATION_DELAY,
};
use std::time::Duration;
use sys::tempfile::tempdir;

struct ExampleRuntime {
    snapshot_secs: u64,
}

impl RuntimeAdapter for ExampleRuntime {
    fn set_snapshot_interval(&mut self, value: Duration) {
        self.snapshot_secs = value.as_secs();
    }
}

fn main() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path().join("gov.db"));

    let mut params = Params::default();
    let mut adapter = ExampleRuntime {
        snapshot_secs: params.snapshot_interval_secs as u64,
    };

    let spec = registry()
        .iter()
        .find(|spec| spec.key == ParamKey::SnapshotIntervalSecs)
        .unwrap();
    let vote_deadline: u64 = 2;
    let activation_epoch = vote_deadline + ACTIVATION_DELAY;

    let proposal = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 45,
        min: spec.min,
        max: spec.max,
        proposer: "ops".into(),
        created_epoch: 0,
        vote_deadline_epoch: vote_deadline,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    };

    let proposal_id = controller::submit_proposal(&store, proposal).unwrap();

    store
        .vote(
            proposal_id,
            Vote {
                proposal_id,
                voter: "ops".into(),
                choice: VoteChoice::Yes,
                weight: 1,
                received_at: 0,
            },
            0,
        )
        .unwrap();

    controller::tally(&store, proposal_id, vote_deadline).unwrap();

    {
        let mut runtime = Runtime::new(&mut adapter);
        controller::activate_ready(&store, activation_epoch, &mut runtime, &mut params).unwrap();
    }

    assert_eq!(params.snapshot_interval_secs, 45);
    assert_eq!(adapter.snapshot_secs, 45);
}
