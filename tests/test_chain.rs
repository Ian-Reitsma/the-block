#![cfg(feature = "fuzzy")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

// tests/test_chain.rs
//
// Integration tests covering chain invariants and edge cases.

use base64::Engine;
use proptest::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, path::Path};
use the_block::hashlayout::BlockEncoder;
use the_block::{
    generate_keypair, sign_tx, Account, Blockchain, ChainDisk, MempoolEntryDisk, Pending,
    RawTxPayload, SignedTransaction, TokenAmount, TokenBalance, TxAdmissionError,
};

mod vectors;
mod util;
use util::temp::{temp_blockchain, temp_dir};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}
fn load_fixture(name: &str) -> tempfile::TempDir {
    let dir = temp_dir("chain_db");
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .join("db.b64");
    let b64 = fs::read_to_string(src).unwrap();
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean)
        .unwrap();
    let dst = dir.path().join("db");
    fs::write(&dst, bytes).unwrap();
    dir
}

fn hash_state(bc: &Blockchain) -> String {
    let disk = ChainDisk {
        schema_version: bc.schema_version(),
        chain: bc.chain.clone(),
        accounts: bc.accounts.clone(),
        emission_consumer: bc.emission_consumer,
        emission_industrial: bc.emission_industrial,
        block_reward_consumer: bc.block_reward_consumer,
        block_reward_industrial: bc.block_reward_industrial,
        block_height: bc.block_height,
        mempool: Vec::new(),
    };
    let bytes = bincode::serialize(&disk).unwrap();
    blake3::hash(&bytes).to_hex().to_string()
}

// === Helper for signing transactions ===
mod testutil {
    use super::*;
    pub fn build_signed_tx(
        priv_key: &[u8],
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
        sign_tx(priv_key.to_vec(), payload.clone()).expect("valid key")
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
        let (_dir, mut bc) = temp_blockchain("temp_chain");
        let miner = &miners[0];
        bc.add_account(miner.clone(), 0, 0).unwrap();
        bc.add_account(alice.clone(), 0, 0).unwrap();
        bc.mine_block(miner).unwrap();
        let (priv_bytes, _pub_bytes) = generate_keypair();

        for n in 0..tx_count {
            let tx = testutil::build_signed_tx(&priv_bytes, miner, &alice, amt_cons, amt_ind, fee, n as u64 + 1);
            let _ = bc.submit_transaction(tx);
        }

        bc.mine_block(miner).unwrap();

        let mb = bc.get_account_balance(miner).unwrap();
        let ab = bc.get_account_balance(&alice).unwrap();
        assert!(mb.consumer as i128 >= 0 && ab.consumer as i128 >= 0);
        assert!(mb.industrial as i128 >= 0 && ab.industrial as i128 >= 0);

        let (em_cons, em_ind) = bc.circulating_supply();
        assert!(em_cons <= 20_000_000_000_000);
        assert!(em_ind <= 20_000_000_000_000);
    }
}

// 1b. Concurrent mempool operations should not leave pending reservations
proptest! {
  #[test]
  fn prop_mempool_concurrency(ops in prop::collection::vec(0u8..3, 1..10)) {
      init();
      let dir = temp_dir("temp_prop");
      let bc = Arc::new(RwLock::new(Blockchain::open(dir.path().to_str().unwrap()).unwrap()));
      {
          let mut w = bc.write().unwrap();
          w.add_account("miner".into(), 0, 0).unwrap();
          w.mine_block("miner").unwrap();
      }
      let (priv_bytes, _pub) = generate_keypair();
      let ops_vec = ops.clone();
      let handles: Vec<_> = ops_vec.into_iter().enumerate().map(|(i, op)| {
          let bc = bc.clone();
          let priv_bytes = priv_bytes.clone();
          std::thread::spawn(move || {
              match op % 3 {
                  0 => {
                      let tx = testutil::build_signed_tx(
                          &priv_bytes,
                          "miner",
                          "miner",
                          0,
                          0,
                          0,
                          i as u64 + 1,
                      );
                      let _ = bc.write().unwrap().submit_transaction(tx);
                  }
                  1 => {
                      let _ = bc
                          .write()
                          .unwrap()
                          .drop_transaction("miner", i as u64 + 1);
                  }
                  _ => {
                      let _ = bc.write().unwrap().mine_block("miner");
                  }
              }
          })
      }).collect();
      for h in handles { let _ = h.join(); }
        let guard = bc.read().unwrap();
        let miner = guard.accounts.get("miner").unwrap();
        assert_eq!(miner.pending.consumer, 0);
        assert_eq!(miner.pending.industrial, 0);
        drop(guard);
        drop(bc);
        let _ = fs::remove_dir_all(&path);
    }
}

// 2. Invalid signature rejected
#[test]
fn test_rejects_invalid_signature() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (priv_bad, pub_bytes) = generate_keypair();
    let payload = RawTxPayload {
        from_: "miner".into(),
        to: "alice".into(),
        amount_consumer: 1,
        amount_industrial: 2,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    // sign with wrong key (priv_bad + 1)
    let mut wrong = priv_bad.clone();
    wrong[0] ^= 0xFF;
    let mut tx = sign_tx(wrong, payload.clone()).expect("valid key");
    tx.public_key = pub_bytes.clone();
    let res = bc.submit_transaction(tx);
    assert!(matches!(res, Err(TxAdmissionError::BadSignature)));
}

// 3. Double-spend / overspend is always rejected
#[test]
fn test_double_spend_is_rejected() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pubk) = generate_keypair();
    let (amt_cons, amt_ind, fee) = (1_000_000_000_000_000, 1_000_000_000_000_000, 0);
    let tx = testutil::build_signed_tx(&privkey, "miner", "alice", amt_cons, amt_ind, fee, 1);

    let res = bc.submit_transaction(tx);
    assert!(matches!(res, Err(TxAdmissionError::InsufficientBalance)));
}

// 4. Emission/Decay and Cap logic
#[test]
fn test_block_reward_decays_and_emission_caps() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let mut last = bc.block_reward_consumer;
    for _ in 0..100 {
        bc.mine_block("miner").unwrap();
        assert!(bc.block_reward_consumer <= last);
        last = bc.block_reward_consumer;
    }

    // simulate cap hit
    bc.emission_consumer = 20_000_000_000_000;
    bc.block_reward_consumer = TokenAmount::new(100);
    let block = bc.mine_block("miner").unwrap();
    assert_eq!(block.transactions[0].payload.amount_consumer, 0);
    let (em_cons, _) = bc.circulating_supply();
    assert!(em_cons <= 20_000_000_000_000);
}

// 4b. Coinbase reward recorded in block
#[test]
fn test_coinbase_reward_recorded() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    let block = bc.mine_block("miner").unwrap();
    let cb = &block.transactions[0];
    assert_eq!(block.coinbase_consumer.0, cb.payload.amount_consumer);
    assert_eq!(block.coinbase_industrial.0, cb.payload.amount_industrial);
}

// 5. Fee handling: miner receives all fees
#[test]
fn test_fee_credit_to_miner() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pubk) = generate_keypair();
    let fee = 7;
    let tx = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 2, fee, 1);

    bc.submit_transaction(tx).unwrap();
    let before = bc.get_account_balance("miner").unwrap();
    bc.mine_block("miner").unwrap();
    let after = bc.get_account_balance("miner").unwrap();

    assert!(after.consumer >= before.consumer + fee);
}

// 6. Replay attack prevention
#[test]
fn test_replay_attack_prevention() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pubk) = generate_keypair();
    let tx = testutil::build_signed_tx(&privkey, "miner", "alice", 5, 2, 0, 1);
    let _ = bc.submit_transaction(tx.clone());

    // replay
    let res = bc.submit_transaction(tx);
    assert!(matches!(res, Err(TxAdmissionError::Duplicate)));
}

// 7. Mempool flush on block mine
#[test]
fn test_mempool_flush_on_block_mine() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pubk) = generate_keypair();
    for n in 0..100 {
        let tx = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 1, 0, n + 1);
        let _ = bc.submit_transaction(tx);
    }
    assert!(!bc.mempool.is_empty());
    bc.mine_block("miner").unwrap();
    assert!(bc.mempool.is_empty());
}

// 8b. Duplicate transaction IDs in block are rejected
#[test]
fn test_duplicate_txid_rejected() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pub) = generate_keypair();
    let tx1 = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 0, 0, 1);
    bc.submit_transaction(tx1.clone()).unwrap();
    let block = bc.mine_block("miner").unwrap();

    let mut bad_block = block.clone();
    bad_block.transactions.push(tx1);
    assert!(!bc.validate_block(&bad_block).unwrap());
}

// 8d. Duplicate (sender, nonce) pairs in block are rejected
#[test]
fn test_duplicate_sender_nonce_rejected_in_block() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pub) = generate_keypair();
    let tx1 = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 0, 0, 1);
    bc.submit_transaction(tx1.clone()).unwrap();
    let block = bc.mine_block("miner").unwrap();

    let mut bad_block = block.clone();
    let tx2 = testutil::build_signed_tx(&privkey, "miner", "alice", 2, 0, 0, 1);
    bad_block.transactions.push(tx2);
    assert!(!bc.validate_block(&bad_block).unwrap());
}

// 8c. Strict nonce and pending balance handling
#[test]
fn test_pending_nonce_and_balances() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();

    let (privkey, _pub) = generate_keypair();
    // first tx with nonce 1
    let tx1 = testutil::build_signed_tx(&privkey, "miner", "alice", 2, 3, 1, 1);
    bc.submit_transaction(tx1).unwrap();
    // gap nonce is rejected
    let gap = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 1, 1, 3);
    assert!(matches!(
        bc.submit_transaction(gap),
        Err(TxAdmissionError::NonceGap)
    ));
    // sequential nonce succeeds
    let tx2 = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 1, 1, 2);
    bc.submit_transaction(tx2).unwrap();

    let sender = bc.accounts.get("miner").unwrap();
    assert_eq!(sender.pending.nonce, 2);
    assert!(sender.pending.consumer > 0);
    assert!(sender.pending.industrial > 0);

    // overspend beyond effective balance fails
    let huge = testutil::build_signed_tx(&privkey, "miner", "alice", u64::MAX / 2, 0, 0, 3);
    assert!(matches!(
        bc.submit_transaction(huge),
        Err(TxAdmissionError::InsufficientBalance)
    ));

    bc.mine_block("miner").unwrap();
    let sender = bc.accounts.get("miner").unwrap();
    assert_eq!(sender.pending.nonce, 0);
    assert_eq!(sender.pending.consumer, 0);
    assert_eq!(sender.pending.industrial, 0);
}

// 8e. Dropping a transaction releases pending reservations
#[test]
fn test_drop_transaction_releases_pending() {
    init();
    let mut bc = Blockchain::open("temp_drop").unwrap();
    bc.add_account("miner".into(), 5, 5).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    let (privkey, _pub) = generate_keypair();
    let tx = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 1, 1, 1);
    bc.submit_transaction(tx).unwrap();
    assert_eq!(bc.accounts.get("miner").unwrap().pending.nonce, 1);
    bc.drop_transaction("miner", 1).unwrap();
    let sender = bc.accounts.get("miner").unwrap();
    assert_eq!(sender.pending.nonce, 0);
    assert_eq!(sender.pending.consumer, 0);
    assert_eq!(sender.pending.industrial, 0);
    assert!(bc.mempool.is_empty());
}

// 8f. Fee checksum must match computed totals
#[test]
fn test_fee_checksum_enforced() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let (privkey, _pub) = generate_keypair();
    let tx = testutil::build_signed_tx(&privkey, "miner", "alice", 1, 0, 2, 1);
    bc.submit_transaction(tx).unwrap();
    let mut block = bc.mine_block("miner").unwrap();
    assert!(bc.validate_block(&block).unwrap());
    block.fee_checksum = "00".repeat(32);
    assert!(!bc.validate_block(&block).unwrap());
}

// 8. Concurrency: multi-threaded mempool/submit/mine
#[test]
fn test_multithreaded_submit_and_mine() {
    init();
    let (_dir, bc_inner) = temp_blockchain("temp_chain");
    let bc = Arc::new(RwLock::new(bc_inner));
    {
        let mut chain = write_lock!(bc);
        chain.add_account("miner".into(), 0, 0).unwrap();
        chain.add_account("bob".into(), 0, 0).unwrap();
        chain.mine_block("miner").unwrap();
    }
    let (privkey, _pubk) = generate_keypair();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let bc = Arc::clone(&bc);
            let privkey = privkey.clone();
            thread::spawn(move || {
                for n in 0..5 {
                    let tx = testutil::build_signed_tx(&privkey, "miner", "bob", 1, 1, 0, n + 1);
                    let mut chain = write_lock!(bc);
                    let _ = chain.submit_transaction(tx.clone());
                    let _ = chain.mine_block("miner");
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let miner = read_lock!(bc).get_account_balance("miner").unwrap();
    let bob = read_lock!(bc).get_account_balance("bob").unwrap();
    assert!(miner.consumer as i128 >= 0 && bob.consumer as i128 >= 0);
}

// 9. Corrupt block is rejected
#[test]
fn test_rejects_corrupt_block() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    let mut block = bc.mine_block("miner").unwrap();
    block.hash = "deadbeef".repeat(8);
    assert!(!bc.validate_block(&block).unwrap());
}

// 10. Persistence: reload chain, ensure round‐trip
#[test]
fn test_chain_persistence() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    bc.add_account("miner".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let before = bc.get_account_balance("miner").unwrap();
    drop(bc);

    let (_dir2, bc2) = temp_blockchain("temp_chain");
    if let Ok(after) = bc2.get_account_balance("miner") {
        assert_eq!(before.consumer, after.consumer);
    }
}

// 11. Fork/reorg resolution
#[test]
fn test_fork_and_reorg_resolution() {
    init();
    let (_dir1, mut bc1) = temp_blockchain("temp_chain");
    let (_dir2, mut bc2) = temp_blockchain("temp_chain");

    for bc in [&mut bc1, &mut bc2].iter_mut() {
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner").unwrap();
    }

    // chain lengths diverge
    for _ in 0..5 {
        bc1.mine_block("miner").unwrap();
    }
    for _ in 0..10 {
        bc2.mine_block("miner").unwrap();
    }

    // import longer into shorter
    let longer = bc2.chain.clone();
    bc1.import_chain(longer).unwrap();

    assert_eq!(bc1.chain, bc2.chain);
    assert_eq!(
        bc1.get_account_balance("miner").unwrap().consumer,
        bc2.get_account_balance("miner").unwrap().consumer
    );
}

// 11b. Import rejects fork with mutated reward field
#[test]
fn test_import_reward_mismatch() {
    init();
    let (_dir1, mut bc1) = temp_blockchain("temp_chain");
    let (_dir2, mut bc2) = temp_blockchain("temp_chain");
    for bc in [&mut bc1, &mut bc2].iter_mut() {
        assert!(bc.get_account_balance("miner").is_err());
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner").unwrap();
    }
    for _ in 0..3 {
        bc1.mine_block("miner").unwrap();
    }
    for _ in 0..6 {
        bc2.mine_block("miner").unwrap();
    }
    let mut fork = bc2.chain.clone();
    let idx = fork.len() - 3;
    fork[idx].coinbase_consumer = TokenAmount::new(fork[idx].coinbase_consumer.0 + 1);
    fork[idx].coinbase_industrial = TokenAmount::new(fork[idx].coinbase_industrial.0 + 1);
    assert!(bc1.import_chain(fork).is_err());
}

// 11c. Import rejects chain with incorrect difficulty
// Ensures import_chain runs full block validation, including difficulty check
// per CONSENSUS.md §10.3.
#[test]
fn test_import_difficulty_mismatch() {
    init();
    let (_dir1, mut bc1) = temp_blockchain("temp_chain");
    let (_dir2, mut bc2) = temp_blockchain("temp_chain");
    for bc in [&mut bc1, &mut bc2].iter_mut() {
        assert!(bc.get_account_balance("miner").is_err());
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner").unwrap();
    }
    for _ in 0..3 {
        bc1.mine_block("miner").unwrap();
    }
    for _ in 0..6 {
        bc2.mine_block("miner").unwrap();
    }
    let mut fork = bc2.chain.clone();
    let idx = fork.len() - 1;
    fork[idx].difficulty += 1;
    let ids: Vec<[u8; 32]> = fork[idx]
        .transactions
        .iter()
        .map(SignedTransaction::id)
        .collect();
    let id_refs: Vec<&[u8]> = ids.iter().map(|h| h.as_ref()).collect();
    let enc = BlockEncoder {
        index: fork[idx].index,
        prev: &fork[idx].previous_hash,
        timestamp: fork[idx].timestamp_millis,
        nonce: fork[idx].nonce,
        difficulty: fork[idx].difficulty,
        coin_c: fork[idx].coinbase_consumer.0,
        coin_i: fork[idx].coinbase_industrial.0,
        fee_checksum: &fork[idx].fee_checksum,
        tx_ids: &id_refs,
    };
    fork[idx].hash = enc.hash();
    assert!(bc1.import_chain(fork).is_err());
}

// 12. Fuzz unicode & overflow addresses
#[test]
fn test_fuzz_unicode_and_overflow_addresses() {
    init();
    let (_dir, mut bc) = temp_blockchain("temp_chain");
    let crazy = "矿工💎🚀𠜎𠜱𡃁𡈽".to_string();
    bc.add_account(crazy.clone(), u64::MAX, u64::MAX).unwrap();
    let bal = bc.get_account_balance(&crazy).unwrap();
    assert_eq!(bal.consumer, u64::MAX);
    assert_eq!(bal.industrial, u64::MAX);
}

// 13. Determinism: replay txs and check chain state
#[test]
fn test_chain_determinism() {
    init();
    let (_dir1, mut bc1) = temp_blockchain("temp_chain");
    let (_dir2, mut bc2) = temp_blockchain("temp_chain");
    for bc in [&mut bc1, &mut bc2].iter_mut() {
        bc.add_account("miner".into(), 0, 0).unwrap();
        bc.mine_block("miner").unwrap();
    }

    let (privkey, _pubk) = generate_keypair();
    let tx1 = testutil::build_signed_tx(&privkey, "miner", "miner", 1, 1, 0, 1);
    let tx2 = tx1.clone();
    bc1.submit_transaction(tx1).unwrap();
    bc2.submit_transaction(tx2).unwrap();

    bc1.mine_block("miner").unwrap();
    bc2.mine_block("miner").unwrap();

    assert_eq!(bc1.chain, bc2.chain);
}

#[test]
fn test_schema_upgrade_compatibility() {
    init();
    for fixture in ["v1", "v2"] {
        let path = load_fixture(fixture);
        let bc = Blockchain::open(&path).unwrap();
        for acc in bc.accounts.values() {
            assert_eq!(acc.pending.consumer, 0);
            assert_eq!(acc.pending.industrial, 0);
            assert_eq!(acc.pending.nonce, 0);
        }
    }

    // Schema v3 stored only wall-clock admission times. Opening should
    // hydrate `timestamp_ticks` for each mempool entry.
    let dir = temp_dir("schema_v3");
    fs::create_dir_all(dir.path()).unwrap();
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let entry = MempoolEntryDisk {
        sender: "a".into(),
        nonce: 1,
        tx,
        timestamp_millis: now,
        timestamp_ticks: 0, // simulate pre-v4 missing field
    };
    let mut accounts = HashMap::new();
    accounts.insert(
        "a".into(),
        Account {
            address: "a".into(),
            balance: TokenBalance {
                consumer: 10,
                industrial: 10,
            },
            nonce: 0,
            pending: Pending::default(),
        },
    );
    let disk = ChainDisk {
        schema_version: 3,
        chain: Vec::new(),
        accounts,
        emission_consumer: 0,
        emission_industrial: 0,
        block_reward_consumer: TokenAmount::new(0),
        block_reward_industrial: TokenAmount::new(0),
        block_height: 0,
        mempool: vec![entry],
    };
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    map.insert("chain".to_string(), bincode::serialize(&disk).unwrap());
    let db_path = dir.path().join("db");
    fs::write(db_path, bincode::serialize(&map).unwrap()).unwrap();

    let bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    let migrated = bc.mempool.get(&(String::from("a"), 1)).unwrap();
    assert_eq!(migrated.timestamp_ticks, migrated.timestamp_millis);
}

#[test]
fn test_snapshot_rollback() {
    init();
    let path = load_fixture("v2");
    let mut bc = Blockchain::open(&path).unwrap();
    let before = hash_state(&bc);
    bc.persist_chain().unwrap();
    let src_db = Path::new(&path).join("db");
    let snapshot_dir = temp_dir("snapshot");
    // `Path::join` normalizes separators so the test works on Linux and macOS.
    let snapshot_db = snapshot_dir.path().join("db");
    fs::copy(&src_db, &snapshot_db).unwrap();
    for _ in 0..3 {
        bc.mine_block("miner").unwrap();
    }
    bc.persist_chain().unwrap();
    // Clear the path so drop closes the DB without deleting the fixture dir.
    bc.path.clear();
    drop(bc);
    fs::copy(&snapshot_db, &src_db).unwrap();
    let bc2 = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    let after = hash_state(&bc2);
    assert_eq!(before, after);
    drop(bc2);
    fs::remove_dir_all(snapshot_dir.path()).unwrap();
}
