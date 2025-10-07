#![cfg(feature = "integration-tests")]
use the_block::compute_market::courier::CourierStore;

#[testkit::tb_serial]
fn courier_receipt_forwarding() {
    let dir = tempfile::tempdir().unwrap();
    let store = CourierStore::open(dir.path().to_str().unwrap());
    let receipt = store.send(b"bundle", "alice");
    assert!(!receipt.acknowledged);
    let forwarded = store.flush(|r| r.sender == "alice").unwrap();
    assert_eq!(forwarded, 1);
    let rec = store.get(receipt.id).unwrap();
    assert!(rec.acknowledged);
}

#[testkit::tb_serial]
fn receipt_validation() {
    let dir = tempfile::tempdir().unwrap();
    let store = CourierStore::open(dir.path().to_str().unwrap());
    store.send(b"payload", "bob");
    assert_eq!(store.flush(|_| false).unwrap(), 0);
    assert_eq!(store.flush(|r| r.sender == "bob").unwrap(), 1);
}

#[cfg(feature = "telemetry")]
#[testkit::tb_serial]
fn courier_retry_updates_metrics() {
    use the_block::telemetry::{COURIER_FLUSH_ATTEMPT_TOTAL, COURIER_FLUSH_FAILURE_TOTAL};

    let attempts_before = COURIER_FLUSH_ATTEMPT_TOTAL.get();
    let failures_before = COURIER_FLUSH_FAILURE_TOTAL.get();

    let dir = tempfile::tempdir().unwrap();
    let store = CourierStore::open(dir.path().to_str().unwrap());
    let receipt = store.send(b"bundle", "alice");
    use std::cell::Cell;
    let first = Cell::new(true);
    let forwarded = store
        .flush(|r| {
            assert!(!store.get(r.id).unwrap().acknowledged);
            if first.get() {
                first.set(false);
                false
            } else {
                true
            }
        })
        .unwrap();
    assert_eq!(forwarded, 1);
    let rec = store.get(receipt.id).unwrap();
    assert!(rec.acknowledged);
    let attempts_delta = COURIER_FLUSH_ATTEMPT_TOTAL.get() - attempts_before;
    let failures_delta = COURIER_FLUSH_FAILURE_TOTAL.get() - failures_before;
    assert_eq!(attempts_delta, 2);
    assert_eq!(failures_delta, 1);
}
