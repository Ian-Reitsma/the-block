#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::sync::Once;
use the_block::Blockchain;
use the_block::{generate_keypair, sign_tx, FeeLane, RawTxPayload, SignedTransaction};

mod util;
use util::temp::temp_dir;

static INIT: Once = Once::new();

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    INIT.call_once(|| {});
}

fn build_tx(sk: &[u8], from: &str, to: &str, fee: u64, nonce: u64) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee,
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid signature")
}

#[testkit::tb_serial]
fn eviction_records_hash_and_releases_slot() {
    init();
    let dir = temp_dir("mempool_eviction");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = 1;
    bc.max_pending_per_account = 1;
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk_a, _) = generate_keypair();
    let (sk_b, _) = generate_keypair();

    let tx1 = build_tx(&sk_a, "alice", "bob", 10, 1);
    let tx1_id = tx1.id();
    bc.submit_transaction(tx1).unwrap();

    let tx2 = build_tx(&sk_b, "bob", "alice", 30, 1);
    bc.submit_transaction(tx2.clone()).unwrap();
    assert!(bc.mempool_consumer.contains_key(&(String::from("bob"), 1)));

    let evictions = bc.mempool_recent_evictions(FeeLane::Consumer);
    assert!(evictions.contains(&tx1_id));

    bc.drop_transaction("bob", 1).unwrap();

    let tx3 = build_tx(&sk_a, "alice", "bob", 15, 1);
    bc.submit_transaction(tx3).unwrap();
    assert!(bc
        .mempool_consumer
        .contains_key(&(String::from("alice"), 1)));
}
