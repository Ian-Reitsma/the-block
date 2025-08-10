use std::sync::atomic::{AtomicUsize, Ordering};
use std::{collections::HashMap, fs, path::Path};
use the_block::{Block, Blockchain, ChainDisk, RawTxPayload, SignedTransaction, TokenAmount};

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
fn migrate_v3_recomputes_supply() {
    init();
    let path = unique_path("schema_v3_recompute");
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();

    let coinbase = SignedTransaction {
        payload: RawTxPayload {
            from_: "cb".into(),
            to: "miner".into(),
            amount_consumer: 50,
            amount_industrial: 25,
            fee: 0,
            fee_selector: 0,
            nonce: 0,
            memo: Vec::new(),
        },
        public_key: vec![],
        signature: vec![],
    };
    let tx = SignedTransaction {
        payload: RawTxPayload {
            from_: "alice".into(),
            to: "bob".into(),
            amount_consumer: 1,
            amount_industrial: 0,
            fee: 100,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        },
        public_key: vec![],
        signature: vec![],
    };
    let block = Block {
        index: 0,
        previous_hash: "0".repeat(64),
        timestamp_millis: 0,
        transactions: vec![coinbase.clone(), tx.clone()],
        difficulty: 1,
        nonce: 0,
        hash: String::new(),
        coinbase_consumer: TokenAmount::new(0),
        coinbase_industrial: TokenAmount::new(0),
        fee_checksum: String::new(),
    };
    let disk = ChainDisk {
        schema_version: 3,
        chain: vec![block],
        accounts: HashMap::new(),
        emission_consumer: 0,
        emission_industrial: 0,
        block_reward_consumer: TokenAmount::new(0),
        block_reward_industrial: TokenAmount::new(0),
        block_height: 1,
        mempool: Vec::new(),
    };
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    map.insert("chain".to_string(), bincode::serialize(&disk).unwrap());
    let db_path = Path::new(&path).join("db");
    fs::write(db_path, bincode::serialize(&map).unwrap()).unwrap();

    let bc = Blockchain::open(&path).unwrap();
    let blk = &bc.chain[0];
    assert_eq!(blk.coinbase_consumer.get(), 50);
    assert_eq!(blk.coinbase_industrial.get(), 25);
    let (fc, fi) = the_block::fee_decompose(0, 100).unwrap();
    let mut h = blake3::Hasher::new();
    h.update(&fc.to_le_bytes());
    h.update(&fi.to_le_bytes());
    assert_eq!(blk.fee_checksum, h.finalize().to_hex().to_string());
    assert_eq!(bc.circulating_supply(), (50, 25));
}
