#![allow(clippy::needless_range_loop)]

use std::fs;
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, mempool_cmp, sign_tx, Blockchain, MempoolEntry, RawTxPayload,
    SignedTransaction, TxAdmissionError,
};

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
fn replacement_rejected() {
    init();
    let dir = temp_dir("temp_replace");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
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
    let dir = temp_dir("temp_evict");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_b, _pk_b) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    let tx2 = build_signed_tx(&sk_b, "bob", "alice", 1, 0, 2000, 1);
    bc.submit_transaction(tx2.clone()).unwrap();
    assert!(bc.mempool.contains_key(&("bob".to_string(), 1)));
    bc.drop_transaction("bob", 1).unwrap();
    let tx3 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx3).unwrap();
}

#[test]
fn ttl_expiry_purges_and_counts() {
    init();
    let dir = temp_dir("temp_ttl");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    if let Some(mut entry) = bc.mempool.get_mut(&("alice".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    let dropped = bc.purge_expired();
    assert_eq!(1, dropped);
    assert!(bc.mempool.is_empty());
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::TTL_DROP_TOTAL.get());
}

#[test]
fn fee_floor_enforced() {
    init();
    let dir = temp_dir("temp_fee_floor");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 0, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
}

#[test]
fn orphan_sweep_removes_missing_sender() {
    init();
    let dir = temp_dir("temp_orphan");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    bc.accounts.remove("alice");
    #[cfg(feature = "telemetry")]
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    let _ = bc.purge_expired();
    assert!(bc.mempool.is_empty());
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::ORPHAN_SWEEP_TOTAL.get());
}

#[test]
fn orphan_ratio_triggers_rebuild() {
    init();
    let dir = temp_dir("temp_orphan_ratio");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    for n in 1..=3 {
        let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, n);
        bc.submit_transaction(tx).unwrap();
    }
    bc.accounts.remove("alice");
    let _ = bc.purge_expired();
    assert_eq!(bc.mempool.len(), 0);
    assert_eq!(bc.orphan_count(), 0);
}

#[test]
fn heap_orphan_stress_triggers_rebuild_and_orders() {
    init();
    let dir = temp_dir("temp_heap_orphan");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 16;
    bc.add_account("sink".into(), 0, 0).unwrap();
    let mut keys = Vec::new();
    for i in 0..7 {
        let name = format!("acct{i}");
        bc.add_account(name.clone(), 10_000, 0).unwrap();
        let (sk, _pk) = generate_keypair();
        keys.push((name, sk));
    }
    for i in 0..7 {
        let tx = build_signed_tx(&keys[i].1, &keys[i].0, "sink", 1, 0, 1000 + i as u64, 1);
        bc.submit_transaction(tx).unwrap();
    }
    for i in 0..4 {
        bc.accounts.remove(&keys[i].0);
    }
    #[cfg(feature = "telemetry")]
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    let _ = bc.purge_expired();
    assert_eq!(bc.mempool.len(), 3);
    assert_eq!(bc.orphan_count(), 0);
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::ORPHAN_SWEEP_TOTAL.get());
    let ttl = bc.tx_ttl;
    let mut entries: Vec<_> = bc.mempool.iter().map(|e| e.value().clone()).collect();
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    for w in entries.windows(2) {
        assert!(mempool_cmp(&w[0], &w[1], ttl) != std::cmp::Ordering::Greater);
    }
}

#[test]
fn orphan_drop_decrements_counter() {
    init();
    let dir = temp_dir("temp_orphan_drop");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("carol".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_c, _pk_c) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    let tx2 = build_signed_tx(&sk_c, "carol", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    bc.submit_transaction(tx2).unwrap();
    bc.accounts.remove("alice");
    let _ = bc.purge_expired();
    assert_eq!(bc.orphan_count(), 1);
    bc.drop_transaction("alice", 1).unwrap();
    assert_eq!(bc.orphan_count(), 0);
}

#[test]
fn ttl_purge_drops_orphan_and_decrements_counter() {
    init();
    let dir = temp_dir("temp_orphan_ttl");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("carol".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_c, _pk_c) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    let tx2 = build_signed_tx(&sk_c, "carol", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    bc.submit_transaction(tx2).unwrap();
    bc.accounts.remove("alice");
    let _ = bc.purge_expired();
    assert_eq!(bc.orphan_count(), 1);
    if let Some(mut entry) = bc.mempool.get_mut(&("alice".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    let dropped = bc.purge_expired();
    assert_eq!(dropped, 1);
    assert_eq!(bc.orphan_count(), 0);
    assert_eq!(bc.mempool.len(), 1);
}

#[test]
fn drop_lock_poisoned_error_and_recovery() {
    init();
    let dir = temp_dir("temp_drop_poison");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    #[cfg(feature = "telemetry")]
    {
        telemetry::LOCK_POISON_TOTAL.reset();
        telemetry::TX_REJECTED_TOTAL.reset();
    }
    bc.poison_mempool();
    assert_eq!(
        bc.drop_transaction("alice", 1),
        Err(TxAdmissionError::LockPoisoned)
    );
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(1, telemetry::LOCK_POISON_TOTAL.get());
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .with_label_values(&["lock_poison"])
                .get()
        );
    }
    bc.heal_mempool();
    assert_eq!(bc.drop_transaction("alice", 1), Ok(()));
}

#[test]
fn submit_lock_poisoned_error_and_recovery() {
    init();
    let dir = temp_dir("temp_submit_poison");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    #[cfg(feature = "telemetry")]
    {
        telemetry::LOCK_POISON_TOTAL.reset();
        telemetry::TX_REJECTED_TOTAL.reset();
    }
    bc.poison_mempool();
    assert_eq!(
        bc.submit_transaction(tx.clone()),
        Err(TxAdmissionError::LockPoisoned)
    );
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(1, telemetry::LOCK_POISON_TOTAL.get());
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .with_label_values(&["lock_poison"])
                .get()
        );
    }
    bc.heal_mempool();
    assert_eq!(bc.submit_transaction(tx), Ok(()));
}

#[test]
fn eviction_panic_rolls_back() {
    init();
    let dir = temp_dir("temp_evict_panic");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_b, _pk_b) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    let tx2 = build_signed_tx(&sk_b, "bob", "alice", 1, 0, 2000, 1);
    bc.panic_next_evict();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bc.submit_transaction(tx2.clone()).unwrap();
    }));
    assert!(res.is_err());
    bc.heal_mempool();
    bc.heal_lock("alice");
    bc.heal_lock("bob");
    assert_eq!(bc.mempool.len(), 0);
    bc.submit_transaction(tx2).unwrap();
    assert_eq!(bc.mempool.len(), 1);
}

#[test]
fn admission_panic_rolls_back() {
    init();
    let dir = temp_dir("temp_admit_panic");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    // step 0: panic before reservation; step 1: panic after reservation
    for step in 0..=1 {
        let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
        bc.panic_in_admission_after(step);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bc.submit_transaction(tx.clone()).unwrap();
        }));
        assert!(res.is_err());
        bc.heal_admission();
        bc.heal_mempool();
        bc.heal_lock("alice");
        assert!(bc.mempool.is_empty());
        let acc = bc.accounts.get("alice").unwrap();
        assert_eq!(acc.pending.consumer, 0);
        assert_eq!(acc.pending.nonce, 0);
        assert!(acc.pending.nonces.is_empty());
    }
}

#[test]
fn comparator_orders_by_fee_expiry_hash() {
    init();
    let ttl = 10;
    let dir = temp_dir("temp_cmp");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = ttl;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "alice", "bob", 1, 0, 2000, 1);
    let tx2 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 2);
    let tx3 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 3);
    let size1 = bincode::serialize(&tx1)
        .map(|b| b.len() as u64)
        .unwrap_or(0);
    let size2 = bincode::serialize(&tx2)
        .map(|b| b.len() as u64)
        .unwrap_or(0);
    let size3 = bincode::serialize(&tx3)
        .map(|b| b.len() as u64)
        .unwrap_or(0);
    let e1 = MempoolEntry {
        tx: tx1,
        timestamp_millis: 1,
        timestamp_ticks: 1,
        serialized_size: size1,
    };
    let e2 = MempoolEntry {
        tx: tx2.clone(),
        timestamp_millis: 1,
        timestamp_ticks: 1,
        serialized_size: size2,
    };
    let e3 = MempoolEntry {
        tx: tx3.clone(),
        timestamp_millis: 1,
        timestamp_ticks: 1,
        serialized_size: size3,
    };
    let mut entries = vec![e3, e2.clone(), e1];
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    assert_eq!(entries[0].tx.payload.nonce, 1);
    let mut ids = [tx2.id(), tx3.id()];
    ids.sort();
    assert_eq!(entries[1].tx.id(), ids[0]);
    assert_eq!(entries[2].tx.id(), ids[1]);
}
