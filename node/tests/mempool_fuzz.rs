#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use foundation_serialization::binary;
use rand::{rngs::StdRng, Rng};
use the_block::{
    generate_keypair, mempool_cmp, sign_tx, Blockchain, RawTxPayload, SignedTransaction,
};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
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
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid key")
}

#[test]
fn fuzz_mempool_random_fees_nonces() {
    init();
    const THREADS: usize = 32;
    const TOTAL_ITERS: usize = 10_000;

    let dir = temp_dir("temp_fuzz");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = 128;
    bc.max_pending_per_account = 128;
    bc.add_account("sink".into(), 0, 0).unwrap();

    let mut accounts = Vec::new();
    let mut nonces = Vec::new();
    for i in 0..THREADS {
        let name = format!("acct{i}");
        bc.add_account(name.clone(), 1_000_000, 0).unwrap();
        let (sk, _pk) = generate_keypair();
        accounts.push((name, sk));
        nonces.push(AtomicU64::new(0));
    }

    let bc = Arc::new(RwLock::new(bc));
    let accounts = Arc::new(accounts);
    let nonces = Arc::new(nonces);
    let records = Arc::new(Mutex::new(Vec::<(u64, String, u64)>::new()));

    let per_thread = TOTAL_ITERS / THREADS;
    let remainder = TOTAL_ITERS % THREADS;
    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let bc_cl = Arc::clone(&bc);
            let acc_cl = Arc::clone(&accounts);
            let nonce_cl = Arc::clone(&nonces);
            let rec_cl = Arc::clone(&records);
            std::thread::spawn(move || {
                let mut rng = StdRng::seed_from_u64(t as u64);
                let local_iters = per_thread + if t < remainder { 1 } else { 0 };
                for _ in 0..local_iters {
                    let idx = rng.gen_range(0..THREADS);
                    let (ref name, ref sk) = acc_cl[idx];
                    let nonce = nonce_cl[idx].fetch_add(1, Ordering::SeqCst) + 1;
                    let fee = rng.gen_range(1..10_000);
                    let tx = build_signed_tx(sk, name, "sink", 1, 0, fee, nonce);
                    if bc_cl
                        .write()
                        .unwrap()
                        .submit_transaction(tx.clone())
                        .is_ok()
                    {
                        let size = binary::encode(&tx).unwrap().len() as u64;
                        let fpb = if size == 0 { 0 } else { fee / size };
                        rec_cl.lock().unwrap().push((fpb, name.clone(), nonce));
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let guard = bc.read().unwrap();
    assert!(guard.mempool_consumer.len() <= guard.max_mempool_size_consumer);

    let mut seen = std::collections::HashSet::new();
    guard.mempool_consumer.for_each(|key, _value| {
        assert!(seen.insert((key.0.clone(), key.1)));
    });

    let rec = records.lock().unwrap().clone();
    let mut mem_keys = std::collections::HashSet::new();
    guard.mempool_consumer.for_each(|key, _value| {
        mem_keys.insert((key.0.clone(), key.1));
    });
    let ttl = guard.tx_ttl;
    let mut entries = Vec::new();
    guard.mempool_consumer.for_each(|_key, value| {
        entries.push(value.clone());
    });
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    for w in entries.windows(2) {
        assert!(mempool_cmp(&w[0], &w[1], ttl) != std::cmp::Ordering::Greater);
    }
    let min_fee = entries
        .last()
        .map(|e| {
            let size = binary::encode(&e.tx).unwrap().len() as u64;
            if size == 0 {
                0
            } else {
                e.tx.payload.fee / size
            }
        })
        .unwrap_or(0);
    for (fpb, sender, nonce) in rec {
        if fpb > min_fee {
            assert!(mem_keys.contains(&(sender, nonce)));
        }
    }
}
