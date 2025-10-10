#![cfg(feature = "integration-tests")]
use the_block::{
    blockchain::process::{commit, validate_and_apply},
    transaction::{sign_tx, RawTxPayload},
    Account, Blockchain, TokenAmount, TokenBalance,
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
                consumer: 100,
                industrial: 0,
            },
            nonce: 0,
            pending_consumer: 0,
            pending_industrial: 0,
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
                consumer: 0,
                industrial: 0,
            },
            nonce: 0,
            pending_consumer: 0,
            pending_industrial: 0,
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
        pct_ct: 100,
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
        pct_ct: 100,
        nonce: 2,
        memo: Vec::new(),
    };
    let tx2 = sign_tx(&sk, &payload2).unwrap();

    let mut block = the_block::Block {
        index: 1,
        previous_hash: String::new(),
        timestamp_millis: 0,
        transactions: vec![tx1.clone(), tx2.clone()],
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

    // Applying block with invalid tx2 should fail and leave state untouched
    assert!(validate_and_apply(&bc, &block).is_err());
    assert_eq!(bc.accounts["alice"].balance.consumer, 100);
    assert!(bc.accounts.get("carol").is_none());

    // Remove invalid tx and reapply
    block.transactions = vec![tx1];
    let deltas = validate_and_apply(&bc, &block).expect("valid block");
    commit(&mut bc, deltas).unwrap();
    assert_eq!(bc.accounts["alice"].balance.consumer, 90);
    assert_eq!(bc.accounts["bob"].balance.consumer, 10);
}
