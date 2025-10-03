use governance::{DisbursementStatus, GovStore};
use tempfile::tempdir;

#[test]
fn treasury_disbursements_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);

    let scheduled = store
        .queue_disbursement("dest-1", 42, "initial memo", 100)
        .expect("queue disbursement");
    assert_eq!(scheduled.id, 1);
    assert!(matches!(scheduled.status, DisbursementStatus::Scheduled));

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

    // ensure persistence across reopen
    drop(store);
    let store = GovStore::open(&db_path);
    let persisted = store.disbursements().expect("list persisted");
    assert_eq!(persisted.len(), 1);
    assert!(matches!(
        persisted[0].status,
        DisbursementStatus::Executed { .. }
    ));

    let scheduled_two = store
        .queue_disbursement("dest-2", 7, "", 200)
        .expect("queue second");
    assert_eq!(scheduled_two.id, 2);

    let cancelled = store
        .cancel_disbursement(scheduled_two.id, "operator request")
        .expect("cancel disbursement");
    match cancelled.status {
        DisbursementStatus::Cancelled { ref reason, .. } => {
            assert_eq!(reason, "operator request");
        }
        other => panic!("unexpected status after cancel: {other:?}"),
    }

    let final_list = store.disbursements().expect("list final");
    assert_eq!(final_list.len(), 2);
    assert!(final_list
        .windows(2)
        .all(|window| window[0].id <= window[1].id));
}
