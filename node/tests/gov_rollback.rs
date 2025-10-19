#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::governance::{GovStore, Params, ProposalStatus, Runtime, ACTIVATION_DELAY};
use the_block::rpc::governance::{gov_propose, gov_vote};
use the_block::Blockchain;

#[testkit::tb_serial]
fn rollback_specific_proposal() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path().join("gov.db"));
    let mut bc = Blockchain::default();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let mut params = Params::default();
    let mut rt = Runtime { bc: &mut bc };
    let prop = gov_propose(
        &store,
        "alice".into(),
        "SnapshotIntervalSecs",
        60,
        5,
        600,
        0,
        1,
    )
    .unwrap_or_else(|_| panic!("propose"));
    let id = prop.id;
    gov_vote(&store, "bob".into(), id, "yes", 0).unwrap_or_else(|_| panic!("vote"));
    assert_eq!(
        store.tally_and_queue(id, 1).unwrap(),
        ProposalStatus::Passed
    );
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 60);
    store
        .rollback_proposal(id, 1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(
        params.snapshot_interval_secs,
        Params::default().snapshot_interval_secs
    );
    Settlement::shutdown();
}
