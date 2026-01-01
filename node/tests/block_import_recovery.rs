#![cfg(feature = "integration-tests")]
#![allow(clippy::unnecessary_get_then_check)]
use std::collections::HashSet;
use std::env;
use std::panic;

use the_block::{
    blockchain::process::{validate_and_apply, ExecutionContext},
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenBalance,
};

struct PreserveGuard;

impl PreserveGuard {
    fn set() -> Self {
        env::set_var("TB_PRESERVE", "1");
        Self
    }
}

impl Drop for PreserveGuard {
    fn drop(&mut self) {
        env::remove_var("TB_PRESERVE");
    }
}

#[test]
fn recovers_from_crash_during_import() {
    let dir = sys::tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    let _preserve = PreserveGuard::set();
    {
        let mut bc = Blockchain::new(&path);
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
        bc.persist_chain().expect("persist initial state");
        let coinbase = the_block::SignedTransaction::default();
        let block = the_block::Block {
            index: 1,
            transactions: vec![coinbase.clone(), tx],
            ..the_block::Block::default()
        };
        let deltas = validate_and_apply(&bc, &block).unwrap();
        let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let mut ctx = ExecutionContext::new(&mut bc);
            ctx.apply(deltas).unwrap();
            panic!("crash");
        }));
    }
    // reopen
    let bc2 = Blockchain::open(&path).expect("reopen blockchain");
    assert_eq!(bc2.accounts["alice"].balance.amount, 100);
    assert!(bc2.accounts.get("bob").is_none());
}
