use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use the_block::{
    generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction, TxAdmissionError,
};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn unique_path(prefix: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{id}")
}

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    consumer: u64,
    industrial: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: consumer,
        amount_industrial: industrial,
        fee,
        fee_selector: 0,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid key")
}

#[test]
fn replacement_rejected() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_replace"));
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "miner", "alice", 1, 1, 1000, 1);
    bc.submit_transaction(tx.clone()).unwrap();
    let res = bc.submit_transaction(tx);
    assert!(matches!(res, Err(TxAdmissionError::Duplicate)));
}

#[test]
fn eviction_via_drop_transaction() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_evict"));
    bc.max_mempool_size = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    let tx2 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 2);
    assert_eq!(
        bc.submit_transaction(tx2),
        Err(TxAdmissionError::MempoolFull)
    );
    bc.drop_transaction("alice", 1).unwrap();
    let tx2 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx2).unwrap();
}

#[test]
fn ttl_expiry_drops_transaction() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_ttl"));
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    bc.drop_transaction("alice", 1).unwrap();
    assert!(bc.mempool.is_empty());
}

#[test]
fn fee_floor_enforced() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_fee_floor"));
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 0, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
}
