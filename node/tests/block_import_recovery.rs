#![cfg(feature = "integration-tests")]
use std::collections::HashSet;
use std::panic;

use the_block::{
    blockchain::process::{validate_and_apply, ExecutionContext},
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenAmount, TokenBalance,
};

#[test]
fn recovers_from_crash_during_import() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();
    {
        let mut bc = Blockchain::new(&path);
        bc.accounts.insert(
            "alice".into(),
            Account {
                address: "alice".into(),
                balance: TokenBalance {
                    consumer: 100,
                    industrial: 0,
                },
                nonce: 0,
                pending_consumer: 0,
                pending_industrial: 0,
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
                pct_ct: 100,
                nonce: 1,
                memo: Vec::new(),
            },
        )
        .unwrap();
        let block = the_block::Block {
            index: 1,
            previous_hash: String::new(),
            timestamp_millis: 0,
            transactions: vec![tx],
            difficulty: 0,
            retune_hint: 0,
            nonce: 0,
            hash: String::new(),
            coinbase_consumer: TokenAmount::new(0),
            coinbase_industrial: TokenAmount::new(0),
            storage_sub_ct: TokenAmount::new(0),
            read_sub_ct: TokenAmount::new(0),
            compute_sub_ct: TokenAmount::new(0),
            proof_rebate_ct: TokenAmount::new(0),
            storage_sub_it: TokenAmount::new(0),
            read_sub_it: TokenAmount::new(0),
            compute_sub_it: TokenAmount::new(0),
            read_root: [0; 32],
            fee_checksum: String::new(),
            state_root: String::new(),
            base_fee: 0,
            l2_roots: Vec::new(),
            l2_sizes: Vec::new(),
            vdf_commit: [0; 32],
            vdf_output: [0; 32],
            vdf_proof: Vec::new(),
        };
        let deltas = validate_and_apply(&bc, &block).unwrap();
        let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let mut ctx = ExecutionContext::new(&mut bc);
            ctx.apply(deltas).unwrap();
            panic!("crash");
        }));
    }
    // reopen
    let bc2 = Blockchain::new(&path);
    assert_eq!(bc2.accounts["alice"].balance.consumer, 100);
    assert!(bc2.accounts.get("bob").is_none());
}
