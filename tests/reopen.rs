#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}

#[test]
fn open_mine_reopen() {
    init();
    let (priv_a, _) = generate_keypair();

    {
        let mut bc = Blockchain::open("chain_db").unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
    }

    let mut bc = Blockchain::open("chain_db").unwrap();
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
    assert!(bc.submit_transaction(tx).is_ok());
}
