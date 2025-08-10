use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{generate_keypair, sign_tx, spawn_purge_loop, Blockchain, RawTxPayload};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn unique_path(prefix: &str) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, AtomicOrdering::Relaxed);
    format!("{prefix}_{id}")
}

#[test]
fn purge_loop_drops_expired_entries() {
    init();
    let path = unique_path("purge_loop");
    let _ = fs::remove_dir_all(&path);
    let mut bc = Blockchain::open(&path).unwrap();
    bc.min_fee_per_byte = 0;
    bc.add_account("a".into(), 10, 10).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    bc.tx_ttl = 1;
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    let bc = Arc::new(Mutex::new(bc));
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = spawn_purge_loop(Arc::clone(&bc), 1, Arc::clone(&shutdown));
    thread::sleep(Duration::from_millis(1100));
    shutdown.store(true, Ordering::SeqCst);
    handle.join().unwrap();
    let guard = bc.lock().unwrap();
    assert!(guard.mempool.is_empty());
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
}

#[test]
#[cfg(feature = "telemetry")]
fn counters_saturate_at_u64_max() {
    init();
    let path = unique_path("purge_saturate");
    let _ = fs::remove_dir_all(&path);
    let mut bc = Blockchain::open(&path).unwrap();
    bc.min_fee_per_byte = 0;
    bc.add_account("a".into(), 10, 10).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    for nonce in 1..=2 {
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1,
            fee_selector: 0,
            nonce,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
    }
    for mut entry in bc.mempool.iter_mut() {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    bc.tx_ttl = 1;
    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::TTL_DROP_TOTAL.inc_by(u64::MAX - 1);
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::TTL_DROP_TOTAL.get());

    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.inc_by(u64::MAX - 1);
    // introduce orphaned transaction
    bc.add_account("c".into(), 10, 10).unwrap();
    let (sk2, _pk2) = generate_keypair();
    let payload = RawTxPayload {
        from_: "c".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk2.to_vec(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    bc.accounts.remove("c");
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::ORPHAN_SWEEP_TOTAL.get());

    // attempt another sweep
    bc.add_account("c".into(), 10, 10).unwrap();
    let payload = RawTxPayload {
        from_: "c".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk2.to_vec(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    bc.accounts.remove("c");
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::ORPHAN_SWEEP_TOTAL.get());

    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
}
