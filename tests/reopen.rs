#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, TxAdmissionError};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

fn unique_path(prefix: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{id}")
}

#[test]
fn open_mine_reopen() {
    init();
    let (priv_a, _) = generate_keypair();
    let path = unique_path("chain_db");
    let _ = fs::remove_dir_all(&path);

    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        // Keep the database directory for the reopen but close handles cleanly.
        bc.path.clear();
    }

    let mut bc = Blockchain::open(&path).unwrap();
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
    assert!(bc.submit_transaction(tx).is_ok());
}

#[test]
fn replay_after_crash_is_duplicate() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("replay_db");
    let _ = fs::remove_dir_all(&path);
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
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
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let mut bc2 = Blockchain::open(&path).unwrap();
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
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    assert_eq!(bc2.submit_transaction(tx), Err(TxAdmissionError::Duplicate));
}

#[test]
fn ttl_expired_purged_on_restart() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("replay_ttl");
    let _ = fs::remove_dir_all(&path);
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.tx_ttl = 1;
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
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
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
            entry.timestamp_millis = 0;
            entry.timestamp_ticks = 0;
        }
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(&path).unwrap();
    assert!(bc2.mempool.is_empty());
}

#[test]
fn timestamp_ticks_persist_across_restart() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("ticks_db");
    let _ = fs::remove_dir_all(&path);
    let first;
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
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
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        first = bc
            .mempool
            .get(&("a".into(), 1))
            .map(|e| e.timestamp_ticks)
            .unwrap();
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(&path).unwrap();
    let persisted = bc2
        .mempool
        .get(&("a".into(), 1))
        .map(|e| e.timestamp_ticks)
        .unwrap();
    assert_eq!(first, persisted);
}
