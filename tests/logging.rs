#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "telemetry")]

use logtest::Logger;
use std::fs;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, ERR_DUPLICATE, ERR_OK};

#[cfg(feature = "telemetry-json")]
use serde_json::Value;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}

#[test]
fn logs_accept_and_reject() {
    init();
    let logger = Logger::start();
    let (priv_a, _) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(priv_a.to_vec(), payload).unwrap();
    let dir = temp_dir("temp_logging");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("a".into(), 10_000, 10_000).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    assert!(bc.submit_transaction(tx.clone()).is_ok());
    assert!(bc.submit_transaction(tx).is_err());

    let logs: Vec<_> = logger.collect();

    #[cfg(feature = "telemetry-json")]
    {
        let mut saw_ok = false;
        let mut saw_dup = false;
        for rec in logs {
            let v: Value = serde_json::from_str(rec.args()).unwrap();
            match (v.get("op"), v.get("reason")) {
                (Some(op), Some(reason)) if op == "admit" && reason == "ok" => {
                    assert_eq!(
                        v.get("code").and_then(Value::as_u64).unwrap(),
                        ERR_OK as u64
                    );
                    saw_ok = true;
                }
                (Some(op), Some(reason)) if op == "reject" && reason == "duplicate" => {
                    assert_eq!(
                        v.get("code").and_then(Value::as_u64).unwrap(),
                        ERR_DUPLICATE as u64
                    );
                    saw_dup = true;
                }
                _ => {}
            }
        }
        assert!(
            saw_ok && saw_dup,
            "missing admit or duplicate log with code"
        );
    }

    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
    {
        assert!(logs.iter().any(|r| r.args().contains("tx accepted")));
        assert!(logs.iter().any(|r| r.args().contains("tx rejected")));
    }
}
