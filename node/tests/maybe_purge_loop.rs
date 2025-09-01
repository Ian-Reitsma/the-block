use std::fs;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, maybe_spawn_purge_loop, sign_tx, Blockchain, RawTxPayload, ShutdownFlag,
};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn env_driven_purge_loop_drops_entries() {
    init();
    let _guard = env_guard();
    let dir = temp_dir("maybe_purge_loop");
    std::env::set_var("TB_PURGE_LOOP_SECS", "1");
    let mut bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    bc.min_fee_per_byte_consumer = 0;
    bc.min_fee_per_byte_industrial = 0;
    bc.base_fee = 0;
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
    if let Some(mut entry) = bc.mempool_consumer.get_mut(&("a".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    bc.tx_ttl = 1;
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    let bc = Arc::new(Mutex::new(bc));
    let shutdown = ShutdownFlag::new();
    let handle =
        maybe_spawn_purge_loop(Arc::clone(&bc), shutdown.as_arc()).expect("invalid interval");
    thread::sleep(Duration::from_millis(50));
    #[cfg(feature = "telemetry")]
    let before = telemetry::TTL_DROP_TOTAL.get();
    shutdown.trigger();
    handle.join().unwrap();
    thread::sleep(Duration::from_millis(50));
    std::env::remove_var("TB_PURGE_LOOP_SECS");
    let guard = bc.lock().unwrap();
    assert!(guard.mempool_consumer.is_empty());
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(1, telemetry::TTL_DROP_TOTAL.get());
        assert_eq!(before, telemetry::TTL_DROP_TOTAL.get());
        telemetry::TTL_DROP_TOTAL.reset();
    }
}

#[test]
fn invalid_env_surfaces_error() {
    init();
    let _guard = env_guard();
    std::env::set_var("TB_PURGE_LOOP_SECS", "not-a-number");
    let dir = temp_dir("invalid_purge_loop");
    let bc = Arc::new(Mutex::new(
        Blockchain::open(dir.path().to_str().unwrap()).unwrap(),
    ));
    let shutdown = ShutdownFlag::new();
    let err = maybe_spawn_purge_loop(Arc::clone(&bc), shutdown.as_arc()).unwrap_err();
    assert!(err.contains("TB_PURGE_LOOP_SECS"));
    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

#[test]
fn zero_env_surfaces_error() {
    init();
    let _guard = env_guard();
    std::env::set_var("TB_PURGE_LOOP_SECS", "0");
    let dir = temp_dir("zero_purge_loop");
    let bc = Arc::new(Mutex::new(
        Blockchain::open(dir.path().to_str().unwrap()).unwrap(),
    ));
    let shutdown = ShutdownFlag::new();
    let err = maybe_spawn_purge_loop(Arc::clone(&bc), shutdown.as_arc()).unwrap_err();
    assert!(err.contains("TB_PURGE_LOOP_SECS"));
    std::env::remove_var("TB_PURGE_LOOP_SECS");
}

#[test]
fn missing_env_surfaces_error() {
    init();
    let _guard = env_guard();
    std::env::remove_var("TB_PURGE_LOOP_SECS");
    let dir = temp_dir("missing_purge_loop");
    let bc = Arc::new(Mutex::new(
        Blockchain::open(dir.path().to_str().unwrap()).unwrap(),
    ));
    let shutdown = ShutdownFlag::new();
    let err = maybe_spawn_purge_loop(Arc::clone(&bc), shutdown.as_arc()).unwrap_err();
    assert!(err.contains("TB_PURGE_LOOP_SECS"));
}

#[test]
fn negative_env_surfaces_error() {
    init();
    let _guard = env_guard();
    std::env::set_var("TB_PURGE_LOOP_SECS", "-5");
    let dir = temp_dir("negative_purge_loop");
    let bc = Arc::new(Mutex::new(
        Blockchain::open(dir.path().to_str().unwrap()).unwrap(),
    ));
    let shutdown = ShutdownFlag::new();
    let err = maybe_spawn_purge_loop(Arc::clone(&bc), shutdown.as_arc()).unwrap_err();
    assert!(err.contains("TB_PURGE_LOOP_SECS"));
    std::env::remove_var("TB_PURGE_LOOP_SECS");
}
