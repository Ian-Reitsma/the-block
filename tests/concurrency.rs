use std::fs;
use std::sync::{Arc, RwLock};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    let _ = fs::remove_dir_all("temp");
    pyo3::prepare_freethreaded_python();
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
fn concurrent_duplicate_submission() {
    init();
    let bc = Arc::new(RwLock::new(Blockchain::new()));
    bc.write()
        .unwrap()
        .add_account("alice".into(), 5, 0)
        .unwrap();
    bc.write().unwrap().add_account("bob".into(), 0, 0).unwrap();
    bc.write().unwrap().mine_block("alice").unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 0, 1);
    let tx_clone = tx.clone();
    let bc1 = Arc::clone(&bc);
    let bc2 = Arc::clone(&bc);
    let t1 = std::thread::spawn(move || bc1.write().unwrap().submit_transaction(tx).is_ok());
    let t2 = std::thread::spawn(move || bc2.write().unwrap().submit_transaction(tx_clone).is_ok());
    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    assert!(r1 ^ r2, "exactly one submission should succeed");
    let pending_nonce = {
        let guard = bc.read().unwrap();
        guard.accounts.get("alice").unwrap().pending.nonce
    };
    assert_eq!(pending_nonce, 1);
    bc.write().unwrap().drop_transaction("alice", 1).unwrap();
    assert!(bc.read().unwrap().mempool.is_empty());
}
