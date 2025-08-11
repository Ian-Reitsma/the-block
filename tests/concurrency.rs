use rand::Rng;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
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
    let dir = temp_dir("temp_concurrency");
    let bc = Arc::new(RwLock::new(Blockchain::new(dir.path().to_str().unwrap())));
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
    let dir = temp_dir("temp_fuzz");
    let bc = Arc::new(RwLock::new(Blockchain::new(dir.path().to_str().unwrap())));
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
    const ITERS: usize = 10_000;
    let handles: Vec<_> = keys
        .into_iter()
        .enumerate()
        .map(|(i, (name, sk))| {
            let bc_cl = Arc::clone(&bc);
            std::thread::spawn(move || {
                let mut rng = rand::thread_rng();
                let to = format!("acc{}", (i + 1) % 32);
                for _ in 0..ITERS {
                    let fee = rng.gen_range(1000..5000);
                    let nonce = rng.gen::<u64>() + 1;
                    let tx = build_signed_tx(&sk, &name, &to, 1, 1, fee, nonce);
                    let _ = bc_cl.write().unwrap().submit_transaction(tx);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let guard = bc.read().unwrap();
    assert!(guard.mempool.len() <= guard.max_mempool_size);
    for acc in guard.accounts.values() {
        assert_eq!(acc.pending.nonce as usize, acc.pending.nonces.len());
    }
}

// Ensure mempool cap is respected even under heavy concurrency.
// CONSENSUS.md §10.3
#[test]
fn cap_race_respects_limit() {
    init();
    let dir = temp_dir("temp_cap_race");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 32;
    bc.max_pending_per_account = 64;
    bc.add_account("alice".into(), 1_000_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    bc.mine_block("alice").unwrap();
    let (sk, _pk) = generate_keypair();
    let bc = Arc::new(RwLock::new(bc));
    let handles: Vec<_> = (0..64)
        .map(|i| {
            let bc_cl = Arc::clone(&bc);
            let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, i as u64 + 1);
            std::thread::spawn(move || {
                let _ = bc_cl.write().unwrap().submit_transaction(tx);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let guard = bc.read().unwrap();
    assert!(guard.mempool.len() <= guard.max_mempool_size);
}

// Flood the mempool from many threads and track the peak size, ensuring the cap is never exceeded.
// AGENTS.md §10.3
#[test]
fn flood_mempool_never_over_cap() {
    init();
    let dir = temp_dir("temp_flood_cap");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 16;
    bc.max_pending_per_account = 64;
    bc.add_account("alice".into(), 1_000_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    bc.mine_block("alice").unwrap();
    let (sk, _pk) = generate_keypair();
    let bc = Arc::new(RwLock::new(bc));
    let peak = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..64)
        .map(|i| {
            let bc_cl = Arc::clone(&bc);
            let peak_cl = Arc::clone(&peak);
            let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, i as u64 + 1);
            std::thread::spawn(move || {
                let _ = bc_cl.write().unwrap().submit_transaction(tx);
                let len = bc_cl.read().unwrap().mempool.len();
                peak_cl.fetch_max(len, Ordering::SeqCst);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let guard = bc.read().unwrap();
    assert!(peak.load(Ordering::SeqCst) <= guard.max_mempool_size);
    assert!(guard.mempool.len() <= guard.max_mempool_size);
}

// Concurrent admission and mining can't push the mempool over its cap.
// AGENTS.md §10.3
#[test]
fn admit_and_mine_never_over_cap() {
    init();
    let dir = temp_dir("temp_admit_mine_cap");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 16;
    bc.max_pending_per_account = 64;
    bc.add_account("alice".into(), 1_000_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    bc.mine_block("alice").unwrap();
    let (sk, _pk) = generate_keypair();
    let bc = Arc::new(RwLock::new(bc));
    let peak = Arc::new(AtomicUsize::new(0));

    // Miner thread repeatedly empties the pool while submissions race.
    let bc_miner = Arc::clone(&bc);
    let peak_miner = Arc::clone(&peak);
    let miner_handle = std::thread::spawn(move || {
        for _ in 0..32 {
            let mut guard = bc_miner.write().unwrap();
            let _ = guard.mine_block("alice");
            let first_ts = guard.chain.first().unwrap().timestamp_millis;
            let len_chain = guard.chain.len() as u64;
            if let Some(last) = guard.chain.last_mut() {
                last.timestamp_millis = first_ts + (len_chain - 1) * 1_000;
            }
            let len = guard.mempool.len();
            drop(guard);
            peak_miner.fetch_max(len, Ordering::SeqCst);
        }
    });

    let handles: Vec<_> = (0..64)
        .map(|i| {
            let bc_cl = Arc::clone(&bc);
            let peak_cl = Arc::clone(&peak);
            let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, i as u64 + 1);
            std::thread::spawn(move || {
                let _ = bc_cl.write().unwrap().submit_transaction(tx);
                let len = bc_cl.read().unwrap().mempool.len();
                peak_cl.fetch_max(len, Ordering::SeqCst);
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    miner_handle.join().unwrap();
    let guard = bc.read().unwrap();
    assert!(peak.load(Ordering::SeqCst) <= guard.max_mempool_size);
    assert!(guard.mempool.len() <= guard.max_mempool_size);
}
