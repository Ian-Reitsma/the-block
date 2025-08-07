use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

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
fn concurrent_duplicate_submission() {
    init();
    let path = unique_path("temp_concurrency");
    let bc = Arc::new(RwLock::new(Blockchain::new(&path)));
    bc.write()
        .unwrap()
        .add_account("alice".into(), 10_000, 0)
        .unwrap();
    bc.write().unwrap().add_account("bob".into(), 0, 0).unwrap();
    bc.write().unwrap().mine_block("alice").unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
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

#[test]
fn cross_thread_fuzz() {
    init();
    let path = unique_path("temp_fuzz");
    let bc = Arc::new(RwLock::new(Blockchain::new(&path)));
    let mut keys = Vec::new();
    for i in 0..32 {
        let name = format!("acc{i}");
        bc.write()
            .unwrap()
            .add_account(name.clone(), 10_000, 10_000)
            .unwrap();
        let (sk, _pk) = generate_keypair();
        keys.push((name, sk));
    }
    let handles: Vec<_> = keys
        .into_iter()
        .enumerate()
        .map(|(i, (name, sk))| {
            let bc_cl = Arc::clone(&bc);
            let to = format!("acc{}", (i + 1) % 32);
            std::thread::spawn(move || {
                let tx = build_signed_tx(&sk, &name, &to, 1, 1, 1000, 1);
                let _ = bc_cl.write().unwrap().submit_transaction(tx);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert!(bc.read().unwrap().mempool.len() <= 32);
}
