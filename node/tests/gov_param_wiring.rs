#![cfg(feature = "telemetry")]
use serial_test::serial;
use std::time::Duration;
use tempfile::tempdir;
use the_block::{
    compute_market::admission,
    generate_keypair,
    governance::{
        GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
        ACTIVATION_DELAY,
    },
    sign_tx, Blockchain, FeeLane, RawTxPayload, TxAdmissionError,
};
#[cfg(feature = "telemetry")]
use the_block::{
    fees::policy,
    telemetry::{PARAM_CHANGE_ACTIVE, PARAM_CHANGE_PENDING},
};

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    consumer: u64,
    industrial: u64,
    fee: u64,
    nonce: u64,
) -> the_block::SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: consumer,
        amount_industrial: industrial,
        fee,
        fee_selector: 1,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("signing")
}

#[test]
#[serial]
fn consumer_fee_comfort_updates_at_epoch_boundary() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("a".into(), 0, 2_000).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    rt.set_consumer_p90_comfort(params.consumer_fee_comfort_p90_microunits as u64);
    rt.set_snapshot_interval(Duration::from_secs(params.snapshot_interval_secs as u64));
    admission::set_min_capacity(params.industrial_admission_min_capacity as u64);
    for _ in 0..50 {
        policy::record_consumer_fee(3000);
    }
    let (sk, _pk) = generate_keypair();
    let mut tx = build_signed_tx(&sk, "a", "b", 0, 1, 1_000, 1);
    tx.lane = FeeLane::Industrial;
    assert_eq!(
        rt.bc.submit_transaction(tx.clone()),
        Err(TxAdmissionError::FeeTooLow)
    );
    let prop = Proposal {
        id: 0,
        key: ParamKey::ConsumerFeeComfortP90Microunits,
        new_value: 4_000,
        min: 500,
        max: 25_000,
        proposer: "g".into(),
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
    #[cfg(feature = "telemetry")]
    assert_eq!(
        PARAM_CHANGE_PENDING
            .with_label_values(&["consumer_fee_comfort_p90_microunits"])
            .get(),
        1
    );
    store.activate_ready(1, &mut rt, &mut params).unwrap();
    assert_eq!(rt.bc.comfort_threshold_p90, 2_500);
    store
        .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
        .unwrap();
    assert_eq!(rt.bc.comfort_threshold_p90, 4_000);
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(
            PARAM_CHANGE_PENDING
                .with_label_values(&["consumer_fee_comfort_p90_microunits"])
                .get(),
            0
        );
        assert_eq!(
            PARAM_CHANGE_ACTIVE
                .with_label_values(&["consumer_fee_comfort_p90_microunits"])
                .get(),
            4_000
        );
    }
    let mut tx2 = build_signed_tx(&sk, "a", "b", 0, 1, 1_000, 1);
    tx2.lane = FeeLane::Industrial;
    let res = rt.bc.submit_transaction(tx2);
    assert!(res.is_ok(), "res={:?}", res);
}

#[test]
#[serial]
fn industrial_min_capacity_param_wires_and_rollback() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    rt.set_consumer_p90_comfort(params.consumer_fee_comfort_p90_microunits as u64);
    rt.set_snapshot_interval(Duration::from_secs(params.snapshot_interval_secs as u64));
    admission::set_min_capacity(params.industrial_admission_min_capacity as u64);
    admission::record_available_shards(10);
    assert!(admission::check_and_record("buyer", "prov", 5).is_ok());
    let prop = Proposal {
        id: 0,
        key: ParamKey::IndustrialAdmissionMinCapacity,
        new_value: 20,
        min: 1,
        max: 10_000,
        proposer: "g".into(),
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
    assert_eq!(admission::min_capacity(), 20);
    assert!(matches!(
        admission::check_and_record("buyer", "prov", 5),
        Err(admission::RejectReason::Capacity)
    ));
    store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut rt, &mut params)
        .unwrap();
    assert_eq!(admission::min_capacity(), 10);
    assert!(admission::check_and_record("buyer", "prov", 5).is_ok());
}

#[test]
#[serial]
fn snapshot_interval_param_updates_runtime() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    rt.set_consumer_p90_comfort(params.consumer_fee_comfort_p90_microunits as u64);
    rt.set_snapshot_interval(Duration::from_secs(params.snapshot_interval_secs as u64));
    admission::set_min_capacity(params.industrial_admission_min_capacity as u64);
    assert_eq!(rt.bc.snapshot.interval, 30);
    let prop = Proposal {
        id: 0,
        key: ParamKey::SnapshotIntervalSecs,
        new_value: 60,
        min: 5,
        max: 600,
        proposer: "g".into(),
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
    assert_eq!(rt.bc.snapshot.interval, 60);
    #[cfg(feature = "telemetry")]
    assert_eq!(
        PARAM_CHANGE_ACTIVE
            .with_label_values(&["snapshot_interval_secs"])
            .get(),
        60
    );
}
