#![allow(clippy::unwrap_used)]
use the_block::governance::{BicameralGovernance as Governance, House};

#[test]
fn submit_vote_exec_cycle() {
    let mut gov = Governance::new(1, 1, 0);
    let id = gov.submit(0, 0, None);
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    assert!(gov.execute(id, 0, None).is_ok());
    let (p, remaining) = gov.status(id, 0).unwrap();
    assert!(p.executed);
    assert_eq!(remaining, 0);
}

#[test]
fn status_reports_timelock() {
    let mut gov = Governance::new(1, 1, 5);
    let id = gov.submit(0, 1, None);
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    let (_, remaining) = gov.status(id, 3).unwrap();
    assert_eq!(remaining, 3);
}

#[cfg(feature = "telemetry")]
#[test]
fn rollback_resets_metrics() {
    use std::time::Duration;
    use tempfile::tempdir;
    use the_block::governance::{
        GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
        ACTIVATION_DELAY,
    };
    use the_block::telemetry::PARAM_CHANGE_ACTIVE;

    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut bc = the_block::Blockchain::new(dir.path().to_str().unwrap());
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    rt.set_snapshot_interval(Duration::from_secs(params.snapshot_interval_secs as u64));
    rt.set_consumer_p90_comfort(params.consumer_fee_comfort_p90_microunits as u64);
    the_block::compute_market::admission::set_min_capacity(
        params.industrial_admission_min_capacity as u64,
    );
    let prop = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 40,
        min: 5,
        max: 600,
        proposer: "a".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
    };
    let pid = store.submit(prop).unwrap();
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
    assert_eq!(
        store.tally_and_queue(pid, 1).unwrap(),
        ProposalStatus::Passed
    );
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(
        PARAM_CHANGE_ACTIVE
            .with_label_values(&["snapshot_interval_secs"])
            .get(),
        40
    );
    store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut rt, &mut params)
        .unwrap();
    assert_eq!(
        PARAM_CHANGE_ACTIVE
            .with_label_values(&["snapshot_interval_secs"])
            .get(),
        30
    );
}

#[test]
fn credit_issue_mints() {
    use credits::Ledger;
    let mut gov = Governance::new(1, 1, 0);
    let id = gov.submit(
        0,
        0,
        Some(the_block::governance::CreditIssue {
            provider: "alice".into(),
            amount: 50,
        }),
    );
    gov.vote(id, House::Operators, true).unwrap();
    gov.vote(id, House::Builders, true).unwrap();
    let mut ledger = Ledger::new();
    gov.execute(id, 0, Some(&mut ledger)).unwrap();
    assert_eq!(ledger.balance("alice"), 50);
}
