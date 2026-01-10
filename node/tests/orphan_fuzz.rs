#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
use std::sync::{Arc, RwLock};

use testkit::tb_prop_test;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

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
    // Validate secret key is exactly 32 bytes for ed25519
    let secret: [u8; 32] = sk
        .try_into()
        .expect("secret key must be 32 bytes for ed25519");
    sign_tx(secret.to_vec(), payload).expect("valid key")
}

#[derive(Clone, Debug)]
enum Op {
    Remove(usize),
    Purge,
}

const ACCOUNTS: usize = 8;

tb_prop_test!(orphan_counter_never_exceeds_mempool, |runner| {
    runner
        .add_random_case("orphan counter", 16, |rng| {
            init();
            // Clear signature cache to avoid any stale entries from previous iterations
            the_block::transaction::clear_signature_cache();
            let dir = temp_dir("temp_orphan_fuzz");
            let mut bc = Blockchain::new(dir.path().to_str().unwrap());
            bc.min_fee_per_byte_consumer = 0;
            bc.min_fee_per_byte_industrial = 0;
            bc.add_account("sink".into(), 0).unwrap();
            for i in 0..ACCOUNTS {
                let name = format!("acc{i}");
                bc.add_account(name.clone(), 1_000_000).unwrap();
                let (sk, pk) = generate_keypair();
                // Verify keypair is valid size
                assert_eq!(sk.len(), 32, "Secret key must be 32 bytes");
                assert_eq!(pk.len(), 32, "Public key must be 32 bytes");
                let tx = build_signed_tx(&sk, &name, "sink", 1, 0, 1_000, 1);
                // Verify embedded public key matches what generate_keypair returned
                assert_eq!(
                    tx.public_key, pk,
                    "Public key mismatch for account {}: embedded != generated",
                    name
                );
                // Verify signature length is correct
                assert_eq!(
                    tx.signature.ed25519.len(), 64,
                    "Signature must be 64 bytes for account {}, got {}",
                    name, tx.signature.ed25519.len()
                );
                // Detailed verification with debug info
                let verify_result = the_block::transaction::verify_signed_tx(&tx);
                if !verify_result {
                    // Re-sign with the same key to see if we get the same result
                    let tx2 = build_signed_tx(&sk, &name, "sink", 1, 0, 1_000, 1);
                    let verify_result2 = the_block::transaction::verify_signed_tx(&tx2);
                    panic!(
                        "Transaction signature verification failed for account {}: \n\
                         sk (first 16 bytes): {:02x?}\n\
                         pk: {:02x?}\n\
                         tx.public_key: {:02x?}\n\
                         signature (first 32 bytes): {:02x?}\n\
                         Re-sign verification: {}\n\
                         tx2.public_key: {:02x?}\n\
                         tx2.signature (first 32 bytes): {:02x?}",
                        name,
                        &sk[..16.min(sk.len())],
                        &pk,
                        &tx.public_key,
                        &tx.signature.ed25519[..32.min(tx.signature.ed25519.len())],
                        verify_result2,
                        &tx2.public_key,
                        &tx2.signature.ed25519[..32.min(tx2.signature.ed25519.len())]
                    );
                }
                bc.submit_transaction(tx)
                    .unwrap_or_else(|e| panic!("Failed to submit transaction for {}: {:?}", name, e));
            }
            let mut keys = Vec::new();
            bc.mempool_consumer.for_each(|key, _value| {
                keys.push(key.clone());
            });
            for key in keys {
                if let Some(mut entry) = bc.mempool_consumer.get_mut(&key) {
                    entry.timestamp_millis = 0;
                }
            }
            let bc = Arc::new(RwLock::new(bc));
            let op_count = rng.range_usize(1..=64);
            let ops: Vec<Op> = (0..op_count)
                .map(|_| {
                    if rng.bool() {
                        let idx = rng.range_usize(0..=ACCOUNTS - 1);
                        Op::Remove(idx)
                    } else {
                        Op::Purge
                    }
                })
                .collect();
            let handles: Vec<_> = ops
                .into_iter()
                .map(|op| {
                    let bc_cl = Arc::clone(&bc);
                    std::thread::spawn(move || match op {
                        Op::Remove(idx) => {
                            let key = format!("acc{idx}");
                            bc_cl.write().unwrap().accounts.remove(&key);
                        }
                        Op::Purge => {
                            let _ = bc_cl.write().unwrap().purge_expired();
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            let guard = bc.read().unwrap();
            let orphans = guard.orphan_count();
            let size = guard.mempool_consumer.len();
            assert_ne!(orphans, usize::MAX);
            assert!(orphans <= size);
        })
        .expect("register random case");
});
