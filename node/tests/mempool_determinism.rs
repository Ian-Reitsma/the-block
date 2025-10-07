#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

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
fn mempool_order_invariant() {
    init();
    let (priv_a, _) = generate_keypair();
    let (priv_b, _) = generate_keypair();
    let tx1 = {
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        };
        sign_tx(priv_a.to_vec(), payload).unwrap()
    };
    let tx2 = {
        let payload = RawTxPayload {
            from_: "b".into(),
            to: "a".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        };
        sign_tx(priv_b.to_vec(), payload).unwrap()
    };

    let dir_a = temp_dir("temp_mempool");
    let dir_b = temp_dir("temp_mempool");
    let mut chain_a = Blockchain::new(dir_a.path().to_str().unwrap());
    let mut chain_b = Blockchain::new(dir_b.path().to_str().unwrap());
    for bc in [&mut chain_a, &mut chain_b].iter_mut() {
        bc.add_account("a".into(), 10_000, 10_000).unwrap();
        bc.add_account("b".into(), 10_000, 10_000).unwrap();
    }

    chain_a.submit_transaction(tx1.clone()).unwrap();
    chain_a.submit_transaction(tx2.clone()).unwrap();
    chain_b.submit_transaction(tx2).unwrap();
    chain_b.submit_transaction(tx1).unwrap();

    let block_a = chain_a.mine_block("miner").unwrap();
    let block_b = chain_b.mine_block("miner").unwrap();
    let mut order_a: Vec<(String, u64)> = block_a
        .transactions
        .iter()
        .skip(1)
        .map(|tx| (tx.payload.from_.clone(), tx.payload.nonce))
        .collect();
    let mut order_b: Vec<(String, u64)> = block_b
        .transactions
        .iter()
        .skip(1)
        .map(|tx| (tx.payload.from_.clone(), tx.payload.nonce))
        .collect();
    order_a.sort();
    order_b.sort();
    assert_eq!(order_a, order_b);
}
