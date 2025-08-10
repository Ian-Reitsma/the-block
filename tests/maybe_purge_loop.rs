use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{generate_keypair, maybe_spawn_purge_loop, sign_tx, Blockchain, RawTxPayload};

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
fn env_driven_purge_loop_drops_entries() {
    init();
    let path = unique_path("maybe_purge_loop");
    let _ = fs::remove_dir_all(&path);
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
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
    let handle =
        maybe_spawn_purge_loop(Arc::clone(&bc), Arc::clone(&shutdown)).expect("loop not started");
    thread::sleep(Duration::from_millis(50));
    shutdown.store(true, Ordering::SeqCst);
    handle.join().unwrap();
    std::env::remove_var("TB_PURGE_LOOP_SECS");
    let guard = bc.lock().unwrap();
    assert!(guard.mempool.is_empty());
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(1, telemetry::TTL_DROP_TOTAL.get());
        telemetry::TTL_DROP_TOTAL.reset();
    }
}
