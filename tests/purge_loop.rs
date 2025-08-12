use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "telemetry")]
use std::sync::Barrier;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

mod util;
use tempfile::TempDir;
use util::temp::temp_dir;

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{generate_keypair, sign_tx, spawn_purge_loop_thread, Blockchain, RawTxPayload};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn prepare_purge_inputs(prefix: &str) -> (TempDir, Blockchain, Vec<u8>) {
    let dir = temp_dir(prefix);
    let mut bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
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
    let tx = sign_tx(sk.clone(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    bc.tx_ttl = 1;
    (dir, bc, sk)
}

#[cfg(feature = "telemetry")]
fn submit_orphan_tx(bc: &mut Blockchain) {
    bc.add_account("c".into(), 10, 10).unwrap();
    let (sk, _pk) = generate_keypair();
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
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    bc.accounts.remove("c");
}

#[test]
fn purge_loop_drops_expired_entries() {
    let _guard = TEST_MUTEX.lock().unwrap();
    init();
    let (_dir, bc, _) = prepare_purge_inputs("purge_loop");
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    let bc = Arc::new(Mutex::new(bc));
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = spawn_purge_loop_thread(Arc::clone(&bc), 1, Arc::clone(&shutdown));
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
    let _guard = TEST_MUTEX.lock().unwrap();
    init();
    let (_dir, mut bc, sk) = prepare_purge_inputs("purge_saturate");
    if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
        entry.timestamp_millis = u64::MAX;
        entry.timestamp_ticks = u64::MAX;
    }
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 2,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.clone(), payload).unwrap();
    bc.submit_transaction(tx).unwrap();
    for mut entry in bc.mempool.iter_mut() {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::TTL_DROP_TOTAL.inc_by(u64::MAX - 1);
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::TTL_DROP_TOTAL.get());

    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.inc_by(u64::MAX - 1);
    submit_orphan_tx(&mut bc);
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::ORPHAN_SWEEP_TOTAL.get());

    // attempt another sweep
    submit_orphan_tx(&mut bc);
    bc.purge_expired();
    assert_eq!(u64::MAX, telemetry::ORPHAN_SWEEP_TOTAL.get());

    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
}

#[test]
#[cfg(feature = "telemetry")]
fn counters_saturate_concurrently() {
    let _guard = TEST_MUTEX.lock().unwrap();
    init();
    const THREADS: usize = 8;
    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    telemetry::TTL_DROP_TOTAL.inc_by(u64::MAX - (THREADS as u64 - 1));
    telemetry::ORPHAN_SWEEP_TOTAL.inc_by(u64::MAX - (THREADS as u64 - 1));
    let start = Arc::new(Barrier::new(THREADS));
    let mid = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let start = Arc::clone(&start);
            let mid = Arc::clone(&mid);
            thread::spawn(move || {
                let (_dir, mut bc, _) = prepare_purge_inputs("concurrent_purge");
                start.wait();
                bc.purge_expired();

                submit_orphan_tx(&mut bc);
                mid.wait();
                bc.purge_expired();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(u64::MAX, telemetry::TTL_DROP_TOTAL.get());
    assert_eq!(u64::MAX, telemetry::ORPHAN_SWEEP_TOTAL.get());
    telemetry::TTL_DROP_TOTAL.reset();
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
}
