use serial_test::serial;
use tempfile::tempdir;
use the_block::governance::{GovStore, Params, Runtime, ProposalStatus, ACTIVATION_DELAY};
use the_block::rpc::governance::{gov_propose, gov_vote};
use the_block::compute_market::settlement::{Settlement, SettleMode};
use the_block::Blockchain;

#[test]
#[serial]
fn credit_decay_governance_updates() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path().join("gov.db"));
    let mut bc = Blockchain::default();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);
    let mut params = Params::default();
    let mut rt = Runtime { bc: &mut bc };
    let prop = gov_propose(
        &store,
        "alice".into(),
        "CreditsDecayLambdaPerHourPpm",
        1000,
        0,
        1_000_000,
        0,
        1,
    )
    .unwrap_or_else(|_| panic!("propose"));
    let id = prop["id"].as_u64().unwrap();
    gov_vote(&store, "bob".into(), id, "yes", 0).unwrap_or_else(|_| panic!("vote"));
    assert_eq!(store.tally_and_queue(id, 1).unwrap(), ProposalStatus::Passed);
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert!((Settlement::decay_lambda() - 0.001).abs() < 1e-6);
    Settlement::shutdown();
}
