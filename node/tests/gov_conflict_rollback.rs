#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::governance::{GovStore, Params, ProposalStatus, Runtime, ACTIVATION_DELAY};
use the_block::rpc::governance::{gov_propose, gov_vote};
use the_block::Blockchain;

#[testkit::tb_serial]
fn rollback_conflicting_proposals() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path().join("gov.db"));
    let mut bc = Blockchain::default();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let mut params = Params::default();
    let mut rt = Runtime { bc: &mut bc };

    // First proposal sets snapshot interval to 60
    let p1 = gov_propose(
        &store,
        "alice".into(),
        "SnapshotIntervalSecs",
        60,
        5,
        600,
        0,
        1,
    )
    .unwrap_or_else(|_| panic!("propose1"));
    let id1 = p1.id;
    gov_vote(&store, "bob".into(), id1, "yes", 0).unwrap_or_else(|_| panic!("vote1"));
    assert_eq!(
        store.tally_and_queue(id1, 1).unwrap(),
        ProposalStatus::Passed
    );
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 60);

    // Second proposal changes value to 30
    let p2 = gov_propose(
        &store,
        "carol".into(),
        "SnapshotIntervalSecs",
        30,
        5,
        600,
        3,
        4,
    )
    .unwrap_or_else(|_| panic!("propose2"));
    let id2 = p2.id;
    gov_vote(&store, "dave".into(), id2, "yes", 3).unwrap_or_else(|_| panic!("vote2"));
    assert_eq!(
        store.tally_and_queue(id2, 4).unwrap(),
        ProposalStatus::Passed
    );
    store
        .activate_ready(4 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 30);

    // Rollback the second proposal and ensure first remains
    store
        .rollback_proposal(id2, 4 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(params.snapshot_interval_secs, 60);
    Settlement::shutdown();
}
