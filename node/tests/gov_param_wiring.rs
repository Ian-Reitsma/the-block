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
    scheduler::{self, current_default_weights, ServiceClass},
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
        pct_ct: 0,
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
    #[cfg(feature = "telemetry")]
    assert_eq!(
        PARAM_CHANGE_ACTIVE
            .with_label_values(&["industrial_admission_min_capacity"])
            .get(),
        20
    );
    store
        .rollback_last(1 + ACTIVATION_DELAY + 1, &mut rt, &mut params)
        .unwrap();
    assert_eq!(admission::min_capacity(), 10);
    assert!(admission::check_and_record("buyer", "prov", 5).is_ok());
    #[cfg(feature = "telemetry")]
    assert_eq!(
        PARAM_CHANGE_ACTIVE
            .with_label_values(&["industrial_admission_min_capacity"])
            .get(),
        10
    );
}

#[test]
#[serial]
fn scheduler_weights_apply_and_record_activation() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    let mut rt = Runtime { bc: &mut bc };
    let mut params = Params::default();
    let initial_weights = current_default_weights();
    let mut expected_old = initial_weights.clone();

    let updates = [
        (ParamKey::SchedulerWeightGossip, 5_i64, ServiceClass::Gossip),
        (
            ParamKey::SchedulerWeightCompute,
            7_i64,
            ServiceClass::Compute,
        ),
        (
            ParamKey::SchedulerWeightStorage,
            4_i64,
            ServiceClass::Storage,
        ),
    ];

    for (idx, (key, new_value, class)) in updates.iter().enumerate() {
        let proposal = Proposal {
            id: idx as u64,
            key: *key,
            new_value: *new_value,
            min: 0,
            max: 16,
            proposer: "gov".into(),
            created_epoch: 0,
            vote_deadline_epoch: 1,
            activation_epoch: None,
            status: ProposalStatus::Open,
        };
        let pid = store.submit(proposal).unwrap();
        store
            .vote(
                pid,
                Vote {
                    proposal_id: pid,
                    voter: "validator".into(),
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
            .activate_ready(1, &mut rt, &mut params)
            .expect("stage activation");
        store
            .activate_ready(1 + ACTIVATION_DELAY, &mut rt, &mut params)
            .expect("activate after delay");

        let weights = current_default_weights();
        assert_eq!(weights.weight(*class), *new_value as u32);
        match key {
            ParamKey::SchedulerWeightGossip => {
                assert_eq!(params.scheduler_weight_gossip, *new_value);
            }
            ParamKey::SchedulerWeightCompute => {
                assert_eq!(params.scheduler_weight_compute, *new_value);
            }
            ParamKey::SchedulerWeightStorage => {
                assert_eq!(params.scheduler_weight_storage, *new_value);
            }
            _ => unreachable!(),
        }

        let record = store
            .last_activation_record()
            .unwrap()
            .expect("recorded activation");
        assert_eq!(record.key, *key);
        assert_eq!(record.new_value, *new_value);
        let expected_previous = match class {
            ServiceClass::Gossip => expected_old.gossip,
            ServiceClass::Compute => expected_old.compute,
            ServiceClass::Storage => expected_old.storage,
        } as i64;
        assert_eq!(record.old_value, expected_previous);

        match class {
            ServiceClass::Gossip => expected_old.gossip = *new_value as u32,
            ServiceClass::Compute => expected_old.compute = *new_value as u32,
            ServiceClass::Storage => expected_old.storage = *new_value as u32,
        }
    }

    scheduler::set_default_weights(
        initial_weights.gossip,
        initial_weights.compute,
        initial_weights.storage,
    );
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
