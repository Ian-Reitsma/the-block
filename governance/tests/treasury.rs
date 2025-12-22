use governance::{
    circuit_breaker::CircuitBreaker, DisbursementDetails, DisbursementPayload, DisbursementStatus,
    GovStore, TreasuryBalanceEventKind, TreasuryDisbursement,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use sys::tempfile::tempdir;

#[test]
fn treasury_disbursements_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);

    assert_eq!(store.treasury_balance().expect("initial balance"), 0);

    let accrual = store
        .record_treasury_accrual(100)
        .expect("accrue treasury balance");
    assert_eq!(accrual.balance, 100);
    assert!(matches!(accrual.event, TreasuryBalanceEventKind::Accrual));
    assert_eq!(
        store.treasury_balance().expect("balance after accrual"),
        100
    );

    let scheduled = store
        .queue_disbursement(DisbursementPayload {
            proposal: Default::default(),
            disbursement: DisbursementDetails {
                destination: "dest-1".into(),
                amount: 42,
                memo: "initial memo".into(),
                scheduled_epoch: 100,
                expected_receipts: Vec::new(),
            },
        })
        .expect("queue disbursement");
    assert_eq!(scheduled.id, 1);
    assert!(matches!(scheduled.status, DisbursementStatus::Draft { .. }));
    assert_eq!(store.treasury_balance().expect("balance after queue"), 100);

    let list = store.disbursements().expect("list disbursements");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].destination, "dest-1");

    let executed = store
        .execute_disbursement(scheduled.id, "0xfeed", Vec::new())
        .expect("execute disbursement");
    match executed.status {
        DisbursementStatus::Executed { ref tx_hash, .. } => {
            assert_eq!(tx_hash, "0xfeed");
        }
        other => panic!("unexpected status after execute: {other:?}"),
    }
    assert_eq!(store.treasury_balance().expect("post execute"), 58);

    // ensure persistence across reopen
    drop(store);
    let store = GovStore::open(&db_path);
    let persisted = store.disbursements().expect("list persisted");
    assert_eq!(persisted.len(), 1);
    assert!(matches!(
        persisted[0].status,
        DisbursementStatus::Executed { .. }
    ));
    assert_eq!(store.treasury_balance().expect("reopened balance"), 58);

    let scheduled_two = store
        .queue_disbursement(DisbursementPayload {
            proposal: Default::default(),
            disbursement: DisbursementDetails {
                destination: "dest-2".into(),
                amount: 7,
                memo: "".into(),
                scheduled_epoch: 200,
                expected_receipts: Vec::new(),
            },
        })
        .expect("queue second");
    assert_eq!(scheduled_two.id, 2);
    assert_eq!(store.treasury_balance().expect("after second queue"), 58);

    let cancelled = store
        .cancel_disbursement(scheduled_two.id, "operator request")
        .expect("cancel disbursement");
    match cancelled.status {
        DisbursementStatus::RolledBack { ref reason, .. } => {
            assert_eq!(reason, "operator request");
        }
        other => panic!("unexpected status after cancel: {other:?}"),
    }
    assert_eq!(store.treasury_balance().expect("after cancel"), 58);

    let final_list = store.disbursements().expect("list final");
    assert_eq!(final_list.len(), 2);
    assert!(final_list
        .windows(2)
        .all(|window| window[0].id <= window[1].id));

    let history = store.treasury_balance_history().expect("balance history");
    assert_eq!(history.last().map(|snap| snap.balance), Some(58));
    assert!(history
        .iter()
        .any(|snap| matches!(snap.event, TreasuryBalanceEventKind::Executed)));
}

#[test]
fn execute_requires_balance() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);

    let scheduled = store
        .queue_disbursement(DisbursementPayload {
            proposal: Default::default(),
            disbursement: DisbursementDetails {
                destination: "dest-1".into(),
                amount: 5,
                memo: "".into(),
                scheduled_epoch: 0,
                expected_receipts: Vec::new(),
            },
        })
        .expect("queue");
    let result = store.execute_disbursement(scheduled.id, "0xbeef", Vec::new());
    assert!(result.is_err(), "expected insufficient balance error");
}

#[test]
fn treasury_executor_stages_and_executes() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store
        .record_treasury_accrual(10_000)
        .expect("fund treasury");
    let scheduled = store
        .queue_disbursement(DisbursementPayload {
            proposal: Default::default(),
            disbursement: DisbursementDetails {
                destination: "dest-exec".into(),
                amount: 1_000,
                memo: "intent".into(),
                scheduled_epoch: 5,
                expected_receipts: Vec::new(),
            },
        })
        .expect("queue disbursement");
    let sign_calls = Arc::new(AtomicUsize::new(0));
    let submit_calls = Arc::new(AtomicUsize::new(0));
    let config = governance::TreasuryExecutorConfig {
        identity: "test-exec".into(),
        poll_interval: Duration::from_millis(25),
        lease_ttl: Duration::from_secs(1),
        epoch_source: Arc::new(|| 10),
        signer: {
            let counter = Arc::clone(&sign_calls);
            Arc::new(move |disbursement| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(governance::SignedExecutionIntent::new(
                    disbursement.id,
                    vec![0xde, 0xad, 0xbe, 0xef],
                    format!("intent-{}", disbursement.id),
                    7,
                ))
            })
        },
        submitter: {
            let counter = Arc::clone(&submit_calls);
            Arc::new(move |intent| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(intent.tx_hash.clone())
            })
        },
        dependency_check: None,
        nonce_floor: Arc::new(AtomicU64::new(0)),
        circuit_breaker: Arc::new(CircuitBreaker::default()),
        circuit_breaker_telemetry: None,
    };
    let handle = store.spawn_treasury_executor(config);

    let mut attempts = 0;
    loop {
        let executed = store
            .disbursements()
            .expect("load disbursements")
            .into_iter()
            .find(|d| d.id == scheduled.id)
            .and_then(|d| match d.status {
                DisbursementStatus::Executed { ref tx_hash, .. } => Some(tx_hash.clone()),
                _ => None,
            });
        if let Some(tx_hash) = executed {
            assert_eq!(tx_hash, format!("intent-{}", scheduled.id));
            break;
        }
        attempts += 1;
        if attempts > 40 {
            panic!("executor did not complete in time");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        sign_calls.load(Ordering::SeqCst),
        1,
        "signer should run exactly once"
    );
    assert!(submit_calls.load(Ordering::SeqCst) >= 1);
    for _ in 0..10 {
        if store.execution_intents().expect("intents").is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(store.execution_intents().expect("intents").is_empty());
    handle.shutdown();
    handle.join();
}

#[test]
fn treasury_executor_reuses_staged_intents() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store.record_treasury_accrual(5_000).expect("fund treasury");
    let disbursement = store
        .queue_disbursement(DisbursementPayload {
            proposal: Default::default(),
            disbursement: DisbursementDetails {
                destination: "reuse".into(),
                amount: 500,
                memo: "memo".into(),
                scheduled_epoch: 1,
                expected_receipts: Vec::new(),
            },
        })
        .expect("queue disbursement");
    store
        .record_execution_intent(governance::SignedExecutionIntent::new(
            disbursement.id,
            vec![0u8; 4],
            "pre-staged".into(),
            9,
        ))
        .expect("stage intent");
    let sign_calls = Arc::new(AtomicUsize::new(0));
    let config = governance::TreasuryExecutorConfig {
        identity: "reuse-exec".into(),
        poll_interval: Duration::from_millis(25),
        lease_ttl: Duration::from_millis(250),
        epoch_source: Arc::new(|| 5),
        signer: {
            let counter = Arc::clone(&sign_calls);
            Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(governance::SignedExecutionIntent::new(
                    0,
                    Vec::new(),
                    String::new(),
                    0,
                ))
            })
        },
        submitter: Arc::new(|intent| Ok(intent.tx_hash.clone())),
        dependency_check: None,
        nonce_floor: Arc::new(AtomicU64::new(0)),
        circuit_breaker: Arc::new(CircuitBreaker::default()),
        circuit_breaker_telemetry: None,
    };
    let handle = store.spawn_treasury_executor(config);
    let mut attempts = 0;
    loop {
        let executed = store
            .disbursements()
            .expect("load disbursements")
            .into_iter()
            .find(|d| d.id == disbursement.id)
            .and_then(|d| match d.status {
                DisbursementStatus::Executed { ref tx_hash, .. } => Some(tx_hash.clone()),
                _ => None,
            });
        if let Some(tx_hash) = executed {
            assert_eq!(tx_hash, "pre-staged");
            break;
        }
        attempts += 1;
        if attempts > 40 {
            panic!("executor did not execute staged intent");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(sign_calls.load(Ordering::SeqCst), 0);
    handle.shutdown();
    handle.join();
}

#[test]
fn executor_leases_coordinate_holders() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    let (lease, acquired) = store
        .refresh_executor_lease("exec-a", Duration::from_millis(100))
        .expect("acquire lease");
    assert!(acquired);
    assert_eq!(lease.holder, "exec-a");
    let (second, acquired_second) = store
        .refresh_executor_lease("exec-b", Duration::from_millis(100))
        .expect("refresh lease");
    assert!(!acquired_second);
    assert_eq!(second.holder, "exec-a");
    store
        .release_executor_lease("exec-a")
        .expect("release lease");
    let (final_lease, acquired_final) = store
        .refresh_executor_lease("exec-b", Duration::from_millis(100))
        .expect("acquire second lease");
    assert!(acquired_final);
    assert_eq!(final_lease.holder, "exec-b");
}

#[test]
fn treasury_executor_records_submission_errors() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store.record_treasury_accrual(2_000)?;
    let disbursement = store.queue_disbursement(DisbursementPayload {
        proposal: Default::default(),
        disbursement: DisbursementDetails {
            destination: "omega".into(),
            amount: 250,
            memo: "{}".into(),
            scheduled_epoch: 0,
            expected_receipts: Vec::new(),
        },
    })?;
    let error_count = Arc::new(AtomicUsize::new(0));
    let config = governance::TreasuryExecutorConfig {
        identity: "error-exec".into(),
        poll_interval: Duration::from_millis(25),
        lease_ttl: Duration::from_millis(250),
        epoch_source: Arc::new(|| 1),
        signer: Arc::new(move |disbursement: &TreasuryDisbursement| {
            Ok(governance::SignedExecutionIntent::new(
                disbursement.id,
                vec![0xde, 0xad],
                format!("err-intent-{}", disbursement.id),
                42,
            ))
        }),
        submitter: {
            let counter = Arc::clone(&error_count);
            Arc::new(move |_intent| {
                counter.fetch_add(1, Ordering::SeqCst);
                Err(governance::TreasuryExecutorError::Submission(
                    "simulated submission failure".into(),
                ))
            })
        },
        dependency_check: None,
        nonce_floor: Arc::new(AtomicU64::new(0)),
        circuit_breaker: Arc::new(CircuitBreaker::default()),
        circuit_breaker_telemetry: None,
    };
    let handle = store.spawn_treasury_executor(config);
    let mut observed_error = None;
    for _ in 0..40 {
        if let Some(snapshot) = store.executor_snapshot()? {
            if snapshot.last_error.is_some() {
                observed_error = Some(snapshot);
                break;
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    handle.shutdown();
    handle.join();
    let snapshot = observed_error.expect("executor did not record submission error");
    assert!(snapshot
        .last_error
        .as_deref()
        .is_some_and(|msg| msg.contains("simulated submission failure")));
    assert!(snapshot.last_submitted_nonce.is_none());
    let staged = store.execution_intent(disbursement.id)?;
    assert!(staged.is_some(), "failed intent should remain staged");
    assert!(error_count.load(Ordering::SeqCst) > 0);
    Ok(())
}

#[test]
fn executor_failover_preserves_nonce_watermark() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store.record_treasury_accrual(5_000)?;

    let first = store.queue_disbursement(DisbursementPayload {
        proposal: Default::default(),
        disbursement: DisbursementDetails {
            destination: "failover-a".into(),
            amount: 250,
            memo: "".into(),
            scheduled_epoch: 0,
            expected_receipts: Vec::new(),
        },
    })?;
    let second = store.queue_disbursement(DisbursementPayload {
        proposal: Default::default(),
        disbursement: DisbursementDetails {
            destination: "failover-b".into(),
            amount: 250,
            memo: "".into(),
            scheduled_epoch: 0,
            expected_receipts: Vec::new(),
        },
    })?;

    let make_config = |identity: &str,
                       gate: Arc<AtomicU64>,
                       dependency: Option<
        Arc<
            dyn Fn(
                    &GovStore,
                    &TreasuryDisbursement,
                ) -> std::result::Result<bool, governance::TreasuryExecutorError>
                + Send
                + Sync,
        >,
    >| {
        governance::TreasuryExecutorConfig {
            identity: identity.into(),
            poll_interval: Duration::from_millis(25),
            lease_ttl: Duration::from_millis(250),
            epoch_source: Arc::new(|| 1),
            signer: {
                let hint = Arc::clone(&gate);
                Arc::new(move |disbursement: &TreasuryDisbursement| {
                    let nonce = hint.load(Ordering::SeqCst).saturating_add(1);
                    Ok(governance::SignedExecutionIntent::new(
                        disbursement.id,
                        Vec::new(),
                        format!("failover-intent-{}", nonce),
                        nonce,
                    ))
                })
            },
            submitter: Arc::new(|intent| Ok(intent.tx_hash.clone())),
            dependency_check: dependency,
            nonce_floor: gate,
            circuit_breaker: Arc::new(CircuitBreaker::default()),
            circuit_breaker_telemetry: None,
        }
    };

    let allow_only_first: Arc<
        dyn Fn(
                &GovStore,
                &TreasuryDisbursement,
            ) -> std::result::Result<bool, governance::TreasuryExecutorError>
            + Send
            + Sync,
    > = Arc::new(move |_store: &GovStore, d: &TreasuryDisbursement| Ok(d.id == first.id));

    let config_a = make_config(
        "exec-a",
        Arc::new(AtomicU64::new(0)),
        Some(allow_only_first.clone()),
    );
    let handle_a = store.spawn_treasury_executor(config_a);

    for _ in 0..40 {
        let disbursements = store.disbursements()?;
        if disbursements
            .iter()
            .any(|d| matches!(d.status, DisbursementStatus::Executed { .. }) && d.id == first.id)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    handle_a.shutdown();
    handle_a.join();

    let config_b = make_config("exec-b", Arc::new(AtomicU64::new(0)), None);
    let handle_b = store.spawn_treasury_executor(config_b);

    for _ in 0..40 {
        let disbursements = store.disbursements()?;
        if disbursements
            .iter()
            .any(|d| matches!(d.status, DisbursementStatus::Executed { .. }) && d.id == second.id)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    handle_b.shutdown();
    handle_b.join();

    let disbursements = store.disbursements()?;
    let mut executed = disbursements
        .into_iter()
        .filter_map(|d| match d.status {
            DisbursementStatus::Executed { tx_hash, .. } => Some((d.id, tx_hash)),
            _ => None,
        })
        .collect::<Vec<_>>();
    executed.sort_by_key(|(id, _)| *id);
    assert_eq!(executed.len(), 2);
    assert_eq!(executed[0].1, "failover-intent-1");
    assert_eq!(executed[1].1, "failover-intent-2");

    let snapshot = store
        .executor_snapshot()
        .expect("load snapshot")
        .expect("snapshot present");
    assert_eq!(snapshot.lease_last_nonce, Some(2));
    assert_eq!(snapshot.last_submitted_nonce, Some(2));

    Ok(())
}
