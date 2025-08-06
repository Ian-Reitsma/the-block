#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "telemetry")]

use logtest::Logger;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}

fn unique_path(prefix: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{id}")
}

#[test]
fn logs_accept_and_reject() {
    init();
    let mut logger = Logger::start();
    let (priv_a, _) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 0,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(priv_a.to_vec(), payload).unwrap();
    let mut bc = Blockchain::new(&unique_path("temp_logging"));
    bc.add_account("a".into(), 10, 10).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    assert!(bc.submit_transaction(tx.clone()).is_ok());
    assert!(logger.any(|r| r.args().contains("tx accepted")));
    assert!(bc.submit_transaction(tx).is_err());
    assert!(logger.any(|r| r.args().contains("tx rejected")));
}
