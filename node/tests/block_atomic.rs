#![cfg(feature = "integration-tests")]
#![allow(clippy::unnecessary_get_then_check)]
use the_block::{
    blockchain::process::{commit, validate_and_apply},
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenBalance,
};

#[test]
fn block_application_is_atomic() {
    let dir = sys::tempfile::tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.accounts.insert(
        "alice".into(),
        Account {
            address: "alice".into(),
            balance: TokenBalance {
                amount: 100,
            },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: std::collections::HashSet::new(),
            sessions: Vec::new(),
        },
    );
    bc.accounts.insert(
        "bob".into(),
        Account {
            address: "bob".into(),
            balance: TokenBalance {
                amount: 0,
            },
            nonce: 0,
            pending_amount: 0,
            pending_nonce: 0,
            pending_nonces: std::collections::HashSet::new(),
            sessions: Vec::new(),
        },
    );

    // sign helper
    let sk = [1u8; 32];
    let payload1 = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 10,
        amount_industrial: 0,
        fee: 0,
        pct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx1 = sign_tx(&sk, &payload1).unwrap();
    let payload2 = RawTxPayload {
        from_: "alice".into(),
        to: "carol".into(),
        amount_consumer: 200,
        amount_industrial: 0,
        fee: 0,
        pct: 100,
        nonce: 2,
        memo: Vec::new(),
    };
    let tx2 = sign_tx(&sk, &payload2).unwrap();

    let coinbase = the_block::SignedTransaction::default();
    let mut block = the_block::Block {
        index: 1,
        transactions: vec![coinbase.clone(), tx1.clone(), tx2.clone()],
        ..the_block::Block::default()
    };

    // Applying block with invalid tx2 should fail and leave state untouched
    assert!(validate_and_apply(&bc, &block).is_err());
    assert_eq!(bc.accounts["alice"].balance.amount, 100);
    assert!(bc.accounts.get("carol").is_none());

    // Remove invalid tx and reapply
    block.transactions = vec![coinbase.clone(), tx1];
    let deltas = validate_and_apply(&bc, &block).expect("valid block");
    commit(&mut bc, deltas).unwrap();
    assert_eq!(bc.accounts["alice"].balance.amount, 90);
    assert_eq!(bc.accounts["bob"].balance.amount, 10);
}
