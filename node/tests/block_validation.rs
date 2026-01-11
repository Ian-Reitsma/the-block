#![cfg(feature = "integration-tests")]
use std::collections::HashSet;
use std::panic;

use the_block::{
    blockchain::process::{validate_and_apply, ExecutionContext},
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenBalance, TxAdmissionError,
};

#[test]
fn rejects_nonce_gap() {
    let dir = sys::tempfile::tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.accounts.insert(
        "alice".into(),
        Account {
            address: "alice".into(),
            balance: TokenBalance { amount: 100 },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        },
    );

    let sk = [1u8; 32];
    let tx1 = sign_tx(
        &sk,
        &RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 10,
            amount_industrial: 0,
            fee: 0,
            pct: 100,
            nonce: 1,
            memo: Vec::new(),
        },
    )
    .unwrap();
    let tx2 = sign_tx(
        &sk,
        &RawTxPayload {
            from_: "alice".into(),
            to: "carol".into(),
            amount_consumer: 5,
            amount_industrial: 0,
            fee: 0,
            pct: 100,
            nonce: 3, // gap (missing nonce 2)
            memo: Vec::new(),
        },
    )
    .unwrap();

    let block = the_block::Block {
        index: 1,
        transactions: vec![tx1, tx2],
        ..the_block::Block::default()
    };

    let res = validate_and_apply(&bc, &block);
    assert!(matches!(res, Err(TxAdmissionError::NonceGap)));
    assert_eq!(bc.accounts["alice"].balance.amount, 100);
}

#[test]
fn rollback_on_mid_block_panic() {
    let dir = sys::tempfile::tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.accounts.insert(
        "alice".into(),
        Account {
            address: "alice".into(),
            balance: TokenBalance { amount: 100 },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
            sessions: Vec::new(),
        },
    );

    let sk = [1u8; 32];
    let tx = sign_tx(
        &sk,
        &RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 10,
            amount_industrial: 0,
            fee: 0,
            pct: 100,
            nonce: 1,
            memo: Vec::new(),
        },
    )
    .unwrap();
    let block = the_block::Block {
        index: 1,
        transactions: vec![tx.clone()],
        ..the_block::Block::default()
    };
    let deltas = validate_and_apply(&bc, &block).unwrap();
    let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let mut ctx = ExecutionContext::new(&mut bc);
        ctx.apply(deltas).unwrap();
        panic!("boom");
    }));
    assert!(res.is_err());
    // state unchanged
    assert_eq!(bc.accounts["alice"].balance.amount, 100);
    assert!(!bc.accounts.contains_key("bob"));
}
