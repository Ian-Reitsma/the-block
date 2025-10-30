use governance::{DisbursementStatus, GovStore, TreasuryBalanceEventKind};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use sys::tempfile::tempdir;

#[test]
fn treasury_disbursements_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);

    assert_eq!(store.treasury_balance().expect("initial balance"), 0);

    let accrual = store
        .record_treasury_accrual(100, 40)
        .expect("accrue treasury balance");
    assert_eq!(accrual.balance_ct, 100);
    assert_eq!(accrual.balance_it, 40);
    assert!(matches!(accrual.event, TreasuryBalanceEventKind::Accrual));
    assert_eq!(
        store.treasury_balance().expect("balance after accrual"),
        100
    );
    assert_eq!(
        store
            .treasury_balances()
            .expect("dual balance after accrual")
            .industrial,
        40
    );

    let scheduled = store
        .queue_disbursement("dest-1", 42, 12, "initial memo", 100)
        .expect("queue disbursement");
    assert_eq!(scheduled.id, 1);
    assert!(matches!(scheduled.status, DisbursementStatus::Scheduled));
    assert_eq!(store.treasury_balance().expect("balance after queue"), 100);
    assert_eq!(
        store
            .treasury_balances()
            .expect("dual balance after queue")
            .industrial,
        40
    );

    let list = store.disbursements().expect("list disbursements");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].destination, "dest-1");

    let executed = store
        .execute_disbursement(scheduled.id, "0xfeed")
        .expect("execute disbursement");
    match executed.status {
        DisbursementStatus::Executed { ref tx_hash, .. } => {
            assert_eq!(tx_hash, "0xfeed");
        }
        other => panic!("unexpected status after execute: {other:?}"),
    }
    assert_eq!(store.treasury_balance().expect("post execute"), 58);
    assert_eq!(
        store
            .treasury_balances()
            .expect("dual balance post execute")
            .industrial,
        28
    );

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
    assert_eq!(
        store
            .treasury_balances()
            .expect("reopened dual balance")
            .industrial,
        28
    );

    let scheduled_two = store
        .queue_disbursement("dest-2", 7, 0, "", 200)
        .expect("queue second");
    assert_eq!(scheduled_two.id, 2);
    assert_eq!(store.treasury_balance().expect("after second queue"), 58);

    let cancelled = store
        .cancel_disbursement(scheduled_two.id, "operator request")
        .expect("cancel disbursement");
    match cancelled.status {
        DisbursementStatus::Cancelled { ref reason, .. } => {
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
    assert_eq!(history.last().map(|snap| snap.balance_ct), Some(58));
    assert_eq!(history.last().map(|snap| snap.balance_it), Some(28));
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
        .queue_disbursement("dest-1", 5, 0, "", 0)
        .expect("queue");
    let result = store.execute_disbursement(scheduled.id, "0xbeef");
    assert!(result.is_err(), "expected insufficient balance error");
}

#[test]
fn treasury_executor_stages_and_executes() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store
        .record_treasury_accrual(10_000, 0)
        .expect("fund treasury");
    let scheduled = store
        .queue_disbursement("dest-exec", 1_000, 0, "intent", 5)
        .expect("queue disbursement");
    let sign_calls = Arc::new(AtomicUsize::new(0));
    let submit_calls = Arc::new(AtomicUsize::new(0));
    let config = governance::TreasuryExecutorConfig {
        poll_interval: Duration::from_millis(25),
        epoch_source: Arc::new(|| 10),
        signer: {
            let counter = Arc::clone(&sign_calls);
            Arc::new(move |disbursement| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(governance::SignedExecutionIntent::new(
                    disbursement.id,
                    vec![0xde, 0xad, 0xbe, 0xef],
                    format!("intent-{}", disbursement.id),
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
    assert!(store.execution_intents().expect("intents").is_empty());
    handle.shutdown();
    handle.join();
}

#[test]
fn treasury_executor_reuses_staged_intents() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store
        .record_treasury_accrual(5_000, 0)
        .expect("fund treasury");
    let disbursement = store
        .queue_disbursement("reuse", 500, 0, "memo", 1)
        .expect("queue disbursement");
    store
        .record_execution_intent(governance::SignedExecutionIntent::new(
            disbursement.id,
            vec![0u8; 4],
            "pre-staged".into(),
        ))
        .expect("stage intent");
    let sign_calls = Arc::new(AtomicUsize::new(0));
    let config = governance::TreasuryExecutorConfig {
        poll_interval: Duration::from_millis(25),
        epoch_source: Arc::new(|| 5),
        signer: {
            let counter = Arc::clone(&sign_calls);
            Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(governance::SignedExecutionIntent::new(
                    0,
                    Vec::new(),
                    String::new(),
                ))
            })
        },
        submitter: Arc::new(|intent| Ok(intent.tx_hash.clone())),
        dependency_check: None,
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
