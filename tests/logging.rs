#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "telemetry")]

use logtest::Logger;
use std::{
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread,
    time::Duration,
};
#[cfg(not(feature = "telemetry-json"))]
use the_block::{
    generate_keypair, sign_tx, spawn_purge_loop_thread, telemetry, Blockchain, RawTxPayload,
};
#[cfg(feature = "telemetry-json")]
use the_block::{
    generate_keypair, sign_tx, spawn_purge_loop_thread, telemetry, Blockchain, RawTxPayload,
    ERR_DUPLICATE, ERR_INSUFFICIENT_BALANCE, ERR_NONCE_GAP, ERR_OK,
};

#[cfg(feature = "telemetry-json")]
use serde_json::Value;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

fn scenario_accept_and_reject(logger: &mut Logger) {
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
    let tx = sign_tx(priv_a.to_vec(), payload.clone()).unwrap();
    let dir = temp_dir("temp_logging");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("a".into(), 10_000, 10_000).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    assert!(bc.submit_transaction(tx.clone()).is_ok());
    assert!(bc.submit_transaction(tx.clone()).is_err());

    let payload_gap = RawTxPayload {
        nonce: 3,
        ..payload.clone()
    };
    let tx_gap = sign_tx(priv_a.clone().to_vec(), payload_gap).unwrap();
    assert!(bc.submit_transaction(tx_gap).is_err());

    let payload_balance = RawTxPayload {
        nonce: 2,
        amount_consumer: 20_000,
        ..payload
    };
    let tx_balance = sign_tx(priv_a.to_vec(), payload_balance).unwrap();
    assert!(bc.submit_transaction(tx_balance).is_err());

    let logs: Vec<_> = logger.collect();

    #[cfg(feature = "telemetry-json")]
    {
        let mut saw_ok = false;
        let mut saw_dup = false;
        let mut saw_nonce = false;
        let mut saw_balance = false;
        for rec in logs {
            let v: Value = serde_json::from_str(rec.args()).unwrap();
            let code = v.get("code").and_then(Value::as_u64).expect("numeric code");
            match (v.get("op"), v.get("reason")) {
                (Some(op), Some(reason)) if op == "admit" && reason == "ok" => {
                    assert_eq!(code, ERR_OK as u64);
                    saw_ok = true;
                }
                (Some(op), Some(reason)) if op == "reject" && reason == "duplicate" => {
                    assert_eq!(code, ERR_DUPLICATE as u64);
                    saw_dup = true;
                }
                (Some(op), Some(reason)) if op == "reject" && reason == "nonce_gap" => {
                    assert_eq!(code, ERR_NONCE_GAP as u64);
                    saw_nonce = true;
                }
                (Some(op), Some(reason)) if op == "reject" && reason == "insufficient_balance" => {
                    assert_eq!(code, ERR_INSUFFICIENT_BALANCE as u64);
                    saw_balance = true;
                }
                _ => {}
            }
        }
        assert!(
            saw_ok && saw_dup && saw_nonce && saw_balance,
            "missing expected telemetry logs",
        );
    }

    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
    {
        assert!(logs.iter().any(|r| r.args().contains("tx accepted")));
        assert!(logs.iter().any(|r| r.args().contains("reason=duplicate")));
        assert!(logs.iter().any(|r| r.args().contains("reason=nonce_gap")));
        assert!(logs
            .iter()
            .any(|r| r.args().contains("reason=insufficient_balance")));
    }
}

fn scenario_purge_loop_counters(logger: &mut Logger) {
    let dir = temp_dir("temp_purge_logs");
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    #[cfg(feature = "telemetry")]
    {
        telemetry::TTL_DROP_TOTAL.reset();
        telemetry::ORPHAN_SWEEP_TOTAL.reset();
    }
    {
        let mut chain = bc.lock().unwrap();
        chain.add_account("a".into(), 10_000, 0).unwrap();
        chain.add_account("b".into(), 0, 0).unwrap();
        chain.tx_ttl = 0;
        let (sk, _pk) = generate_keypair();
        let tx = sign_tx(
            sk.to_vec(),
            RawTxPayload {
                from_: "a".into(),
                to: "b".into(),
                amount_consumer: 1,
                amount_industrial: 0,
                fee: 1000,
                fee_selector: 0,
                nonce: 1,
                memo: Vec::new(),
            },
        )
        .unwrap();
        chain.submit_transaction(tx).unwrap();
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = spawn_purge_loop_thread(bc.clone(), 1, shutdown.clone());
    thread::sleep(Duration::from_millis(100));
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();

    {
        let mut chain = bc.lock().unwrap();
        chain.tx_ttl = 1800;
        chain.add_account("c".into(), 10_000, 0).unwrap();
        let (sk, _pk) = generate_keypair();
        let tx = sign_tx(
            sk.to_vec(),
            RawTxPayload {
                from_: "c".into(),
                to: "b".into(),
                amount_consumer: 1,
                amount_industrial: 0,
                fee: 1000,
                fee_selector: 0,
                nonce: 1,
                memo: Vec::new(),
            },
        )
        .unwrap();
        chain.submit_transaction(tx).unwrap();
        chain.accounts.remove("c");
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = spawn_purge_loop_thread(bc, 1, shutdown.clone());
    thread::sleep(Duration::from_millis(100));
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    handle.join().unwrap();

    let logs: Vec<_> = logger.collect();

    #[cfg(feature = "telemetry-json")]
    {
        let mut saw_ttl = false;
        let mut saw_orphan = false;
        for rec in logs {
            if let Ok(v) = serde_json::from_str::<Value>(rec.args()) {
                if v.get("op") == Some(&Value::String("purge_loop".into())) {
                    if v.get("reason") == Some(&Value::String("ttl_drop_total".into()))
                        && v.get("fpb").and_then(Value::as_u64) == Some(1)
                    {
                        assert_eq!(
                            v.get("code").and_then(Value::as_u64).unwrap(),
                            ERR_OK as u64
                        );
                        saw_ttl = true;
                    }
                    if v.get("reason") == Some(&Value::String("orphan_sweep_total".into()))
                        && v.get("fpb").and_then(Value::as_u64) == Some(1)
                    {
                        assert_eq!(
                            v.get("code").and_then(Value::as_u64).unwrap(),
                            ERR_OK as u64
                        );
                        saw_orphan = true;
                    }
                }
            }
        }
        assert!(saw_ttl && saw_orphan, "missing purge-loop telemetry logs");
    }

    #[cfg(all(feature = "telemetry", not(feature = "telemetry-json")))]
    {
        assert_eq!(
            logs.iter()
                .filter(|r| r.args().contains("ttl_drop_total="))
                .count(),
            1
        );
        assert_eq!(
            logs.iter()
                .filter(|r| r.args().contains("orphan_sweep_total="))
                .count(),
            1
        );
    }
}

#[test]
fn logs_accept_and_reject_and_purge_loop_counters() {
    init();
    let mut logger = Logger::start();
    scenario_accept_and_reject(&mut logger);
    scenario_purge_loop_counters(&mut logger);
}
