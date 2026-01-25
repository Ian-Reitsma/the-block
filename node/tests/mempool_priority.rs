#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::sync::Once;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

#[path = "util/temp.rs"]
mod temp;
use temp::temp_dir;

static PY_INIT: Once = Once::new();
fn init() {
    let _ = fs::remove_dir_all("chain_db");
    PY_INIT.call_once(|| {});
}

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    fee: u64,
    nonce: u64,
) -> the_block::SignedTransaction {
    let payload = RawTxPayload {
        from_: from.into(),
        to: to.into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee,
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    // Validate secret key is exactly 32 bytes for ed25519
    let secret: [u8; 32] = sk
        .try_into()
        .expect("secret key must be 32 bytes for ed25519");
    sign_tx(secret.to_vec(), payload).expect("valid key")
}

#[testkit::tb_serial]
fn eviction_keeps_high_fee() {
    init();
    let dir = temp_dir("mp_evict");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = 1;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 10_000).unwrap();
    let (ska, _) = generate_keypair();
    let (skb, _) = generate_keypair();
    let low = build_signed_tx(&ska, "alice", "bob", 1000, 1);
    let high = build_signed_tx(&skb, "bob", "alice", 5000, 1);
    bc.submit_transaction(low).unwrap();
    bc.submit_transaction(high).unwrap();
    assert!(bc.mempool_consumer.contains_key(&("bob".to_string(), 1)));
    assert!(!bc.mempool_consumer.contains_key(&("alice".to_string(), 1)));
}

#[testkit::tb_serial]
fn block_sorts_by_fee() {
    init();
    let dir = temp_dir("mp_sort");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("miner".into(), 0).unwrap();
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 10_000).unwrap();
    let (ska, _) = generate_keypair();
    let (skb, _) = generate_keypair();
    let low = build_signed_tx(&ska, "alice", "miner", 1000, 1);
    let high = build_signed_tx(&skb, "bob", "miner", 5000, 1);
    bc.submit_transaction(low).unwrap();
    bc.submit_transaction(high).unwrap();
    let block = bc.mine_block("miner").unwrap();
    assert_eq!(block.transactions[1].payload.from_, "bob");
}
