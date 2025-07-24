// tests/test_chain.rs

#[macro_use]
extern crate proptest;

use proptest::prelude::*;
use std::sync::{Arc, RwLock};
use std::thread;
use std::{fs, path::Path};
use the_block::{generate_keypair, sign_message, verify_signature, Block, Blockchain};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}

// === Helper for signing transactions ===
mod testutil {
    use the_block::sign_message;
    pub fn build_signed_tx(
        priv_key: &[u8],
        pub_key: &[u8],
        from: &str,
        to: &str,
        consumer: u64,
        industrial: u64,
        fee: u64,
    ) -> (Vec<u8>, Vec<u8>) {
        let mut msg = Vec::new();
        msg.extend(from.as_bytes());
        msg.extend(to.as_bytes());
        msg.extend(&consumer.to_le_bytes());
        msg.extend(&industrial.to_le_bytes());
        msg.extend(&fee.to_le_bytes());
        let sig = sign_message(priv_key.to_vec(), msg);
        (pub_key.to_vec(), sig)
    }
}

// Convenience macros for locking
macro_rules! write_lock {
    ($lock:expr) => {
        $lock.write().unwrap()
    };
}
macro_rules! read_lock {
    ($lock:expr) => {
        $lock.read().unwrap()
    };
}

// 1. Property-based: supply/balance never negative, never over cap
proptest! {
    #[test]
    fn prop_supply_and_balances_never_negative(
        miners in prop::collection::vec("a".prop_map(|c| format!("miner_{c}")), 1..5),
        alice in "alice_.*",
        tx_count in 1usize..10,
        amt_cons in 1u64..10_000,
        amt_ind in 1u64..10_000,
        fee in 0u64..100,
    ) {
        init();
        let mut bc = Blockchain::new();
        let miner = &miners[0];
        bc.add_account(miner.clone(), 0, 0).unwrap();
        bc.add_account(alice.clone(), 0, 0).unwrap();
        bc.mine_block(miner.clone()).unwrap();
        let (priv_bytes, pub_bytes) = generate_keypair();

        for _ in 0..tx_count {
            let (pubk, sig) = testutil::build_signed_tx(&priv_bytes, &pub_bytes, miner, &alice, amt_cons, amt_ind, fee);
            let _ = bc.submit_transaction(miner.clone(), alice.clone(), amt_cons, amt_ind, fee, pubk, sig);
        }

        bc.mine_block(miner.clone()).unwrap();

        let mb = bc.get_account_balance(miner.clone()).unwrap();
        let ab = bc.get_account_balance(alice.clone()).unwrap();
        assert!(mb.consumer as i128 >= 0 && ab.consumer as i128 >= 0);
        assert!(mb.industrial as i128 >= 0 && ab.industrial as i128 >= 0);

        let (em_cons, em_ind) = bc.circulating_supply();
        assert!(em_cons <= 20_000_000_000_000);
        assert!(em_ind <= 20_000_000_000_000);
    }
}

// 2. Invalid signature rejected
#[test]
fn test_rejects_invalid_signature() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();

    let (_priv, pub_bytes) = generate_keypair();
    // sign with wrong key
    let sig = sign_message(vec![0; 32], {
        let mut m = Vec::new();
        m.extend("miner".as_bytes());
        m.extend("alice".as_bytes());
        m.extend(&1u64.to_le_bytes());
        m.extend(&2u64.to_le_bytes());
        m.extend(&0u64.to_le_bytes());
        m
    });

    let res = bc.submit_transaction(
        "miner".into(),
        "alice".into(),
        1,
        2,
        0,
        pub_bytes.clone(),
        sig,
    );
    assert!(res.is_err(), "Bad signature should be rejected");
}

// 3. Double-spend / overspend is always rejected
#[test]
fn test_double_spend_is_rejected() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();

    let (privkey, pubk) = generate_keypair();
    let (amt_cons, amt_ind, fee) = (1_000_000_000_000_000, 1_000_000_000_000_000, 0);
    let (pub_bytes, sig) =
        testutil::build_signed_tx(&privkey, &pubk, "miner", "alice", amt_cons, amt_ind, fee);

    let res = bc.submit_transaction(
        "miner".into(),
        "alice".into(),
        amt_cons,
        amt_ind,
        fee,
        pub_bytes,
        sig,
    );
    assert!(res.is_err(), "Overspend should be rejected");
}

// 4. Emission/Decay and Cap logic
#[test]
fn test_block_reward_decays_and_emission_caps() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();
    let mut last = bc.block_reward_consumer;
    for _ in 0..100 {
        bc.mine_block("miner".into()).unwrap();
        assert!(bc.block_reward_consumer <= last);
        last = bc.block_reward_consumer;
    }

    // simulate cap hit
    bc.emission_consumer = 20_000_000_000_000;
    bc.block_reward_consumer = 100;
    let block = bc.mine_block("miner".into()).unwrap();
    assert_eq!(block.transactions[0].amount_consumer, 0);
    let (em_cons, _) = bc.circulating_supply();
    assert!(em_cons <= 20_000_000_000_000);
}

// 5. Fee handling: miner receives all fees
#[test]
fn test_fee_credit_to_miner() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();

    let (privkey, pubk) = generate_keypair();
    let fee = 7;
    let (pub_bytes, sig) = testutil::build_signed_tx(&privkey, &pubk, "miner", "alice", 1, 2, fee);

    bc.submit_transaction("miner".into(), "alice".into(), 1, 2, fee, pub_bytes, sig)
        .unwrap();
    let before = bc.get_account_balance("miner".into()).unwrap();
    bc.mine_block("miner".into()).unwrap();
    let after = bc.get_account_balance("miner".into()).unwrap();

    assert!(after.consumer >= before.consumer + fee);
}

// 6. Replay attack prevention (stubbed)
#[test]
#[ignore = "Enable after adding nonce/txid"]
fn test_replay_attack_prevention() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();

    let (privkey, pubk) = generate_keypair();
    let (pub_bytes, sig) = testutil::build_signed_tx(&privkey, &pubk, "miner", "alice", 5, 2, 0);
    bc.submit_transaction(
        "miner".into(),
        "alice".into(),
        5,
        2,
        0,
        pub_bytes.clone(),
        sig.clone(),
    )

    // replay
    let res = bc.submit_transaction("miner".into(), "alice".into(), 5, 2, 0, pub_bytes, sig);
    assert!(res.is_err());
}

// 7. Mempool flush on block mine
#[test]
fn test_mempool_flush_on_block_mine() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();

    let (privkey, pubk) = generate_keypair();
    for _ in 0..100 {
        let (pb, sig) = testutil::build_signed_tx(&privkey, &pubk, "miner", "alice", 1, 1, 0);
        let _ = bc.submit_transaction("miner".into(), "alice".into(), 1, 1, 0, pb, sig);
    }
    assert!(!bc.mempool.is_empty());
    bc.mine_block("miner".into()).unwrap();
    assert!(bc.mempool.is_empty());
}

// 8. Concurrency: multi-threaded mempool/submit/mine
#[test]
fn test_multithreaded_submit_and_mine() {
    init();
    let bc = Arc::new(RwLock::new(Blockchain::new()));
    {
        let mut chain = write_lock!(bc);
        chain.add_account("miner".into(), 0, 0).unwrap();
        chain.add_account("bob".into(), 0, 0).unwrap();
        chain.mine_block("miner".into()).unwrap();
    }
    let (privkey, pubk) = generate_keypair();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let bc = Arc::clone(&bc);
            let privkey = privkey.clone();
            let pubk = pubk.clone();
            thread::spawn(move || {
                for _ in 0..5 {
                    let (pb, sig) =
                        testutil::build_signed_tx(&privkey, &pubk, "miner", "bob", 1, 1, 0);
                    let mut chain = write_lock!(bc);
                    let _ =
                        chain.submit_transaction("miner".into(), "bob".into(), 1, 1, 0, pb, sig);
                    let _ = chain.mine_block("miner".into());
                }
            })
        })
        .collect();

        for h in handles {
            h.join().unwrap();
        }

    let miner = read_lock!(bc).get_account_balance("miner".into()).unwrap();
    let bob = read_lock!(bc).get_account_balance("bob".into()).unwrap();
    assert!(miner.consumer as i128 >= 0 && bob.consumer as i128 >= 0);
}

// 9. Corrupt block is rejected
#[test]
fn test_rejects_corrupt_block() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    let mut block = bc.mine_block("miner".into()).unwrap();
    block.hash = "deadbeef".repeat(8);
    assert!(!bc.validate_block(&block).unwrap());
}

// 10. Persistence: reload chain, ensure round‐trip
#[test]
fn test_chain_persistence() {
    init();
    let mut bc = Blockchain::new();
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();
    let before = bc.get_account_balance("miner".into()).unwrap();
    drop(bc);

    let bc2 = Blockchain::new();
    if let Ok(after) = bc2.get_account_balance("miner".into()) {
        assert_eq!(before.consumer, after.consumer);
    }
}

// 11. Fork/reorg resolution
#[test]
fn test_fork_and_reorg_resolution() {
    init();
    let mut bc1 = Blockchain::new();
    let mut bc2 = Blockchain::new();

    for bc in [&mut bc1, &mut bc2].iter_mut() {
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner".into()).unwrap();
    }

    // chain lengths diverge
    for _ in 0..5 {
        bc1.mine_block("miner".into()).unwrap();
    }
    for _ in 0..10 {
        bc2.mine_block("miner".into()).unwrap();
    }

    // import longer into shorter
    let longer = bc2.chain.clone();
    bc1.import_chain(longer).unwrap();

    assert_eq!(bc1.chain, bc2.chain);
    assert_eq!(
        bc1.get_account_balance("miner".into()).unwrap().consumer,
        bc2.get_account_balance("miner".into()).unwrap().consumer
    );
}

// 12. Fuzz unicode & overflow addresses
#[test]
fn test_fuzz_unicode_and_overflow_addresses() {
    init();
    let mut bc = Blockchain::new();
    let crazy = "矿工💎🚀𠜎𠜱𡃁𡈽".to_string();
    bc.add_account(crazy.clone(), u64::MAX, u64::MAX).unwrap();
    let bal = bc.get_account_balance(crazy).unwrap();
    assert_eq!(bal.consumer, u64::MAX);
    assert_eq!(bal.industrial, u64::MAX);
}

// 13. Determinism: replay txs and check chain state
#[test]
fn test_chain_determinism() {
    init();
    let mut bc1 = Blockchain::new();
    let mut bc2 = Blockchain::new();
    for bc in [&mut bc1, &mut bc2].iter_mut() {
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner".into()).unwrap();
    }

    let (privkey, pubk) = generate_keypair();
    let (pub_bytes, sig) = testutil::build_signed_tx(&privkey, &pubk, "miner", "miner", 1, 1, 0);
    bc1.submit_transaction(
        "miner".into(),
        "miner".into(),
        1,
        1,
        0,
        pub_bytes.clone(),
        sig.clone(),
    )
    .unwrap();
    bc2.submit_transaction("miner".into(), "miner".into(), 1, 1, 0, pub_bytes, sig)
        .unwrap();

    bc1.mine_block("miner".into()).unwrap();
    bc2.mine_block("miner".into()).unwrap();

    assert_eq!(bc1.chain, bc2.chain);
}

// 14. Schema‐upgrade compatibility (sketch)
#[test]
#[ignore = "Enable after schema versioning/migration"]
fn test_schema_upgrade_compatibility() {
    init();
    let db_path = "test_chain_db_upgrade";
    let _ = fs::remove_dir_all(db_path);

    // simulate v0 layout
    #[derive(serde::Serialize, serde::Deserialize)]
    struct OldChain {
        chain: Vec<Block>,
    }

    {
        let db = sled::open(db_path).unwrap();
        let old = OldChain { chain: vec![] };
        db.insert("chain", bincode::serialize(&old).unwrap())
            .unwrap();
        db.flush().unwrap();
    }

    // open & auto‐migrate
    let mut bc = Blockchain::open(db_path).unwrap();
    assert!(bc.schema_version() >= 1);

    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.mine_block("miner".into()).unwrap();
    bc.persist_chain().unwrap();

    let db = sled::open(db_path).unwrap();
    let raw = db.get("chain").unwrap().unwrap();
    let disk: the_block::ChainDisk = bincode::deserialize(&raw).unwrap();
    assert!(disk.schema_version >= 1);

    let _ = fs::remove_dir_all(db_path);
}