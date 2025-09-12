use explorer::Explorer;
use tempfile::tempdir;
use the_block::{
    transaction::{FeeLane, RawTxPayload, SignedTransaction},
    Block, TokenAmount,
};

#[test]
fn index_block_and_search() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();

    let payload = RawTxPayload::new(
        "alice".into(),
        "contract".into(),
        1,
        0,
        0,
        100,
        1,
        b"memo".to_vec(),
    );
    let tx = SignedTransaction::new(payload, vec![], vec![], FeeLane::Consumer);
    let block = Block {
        index: 1,
        previous_hash: String::new(),
        timestamp_millis: 0,
        transactions: vec![tx],
        difficulty: 0,
        nonce: 0,
        hash: "b1".into(),
        coinbase_consumer: TokenAmount(0),
        coinbase_industrial: TokenAmount(0),
        storage_sub_ct: TokenAmount(0),
        read_sub_ct: TokenAmount(0),
        compute_sub_ct: TokenAmount(0),
        storage_sub_it: TokenAmount(0),
        read_sub_it: TokenAmount(0),
        compute_sub_it: TokenAmount(0),
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
    ex.index_block(&block).unwrap();
    assert!(ex.get_block("b1").unwrap().is_some());
    assert_eq!(ex.search_memo("memo").unwrap().len(), 1);
    assert_eq!(ex.search_contract("contract").unwrap().len(), 1);
}
