use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, mempool_cmp, sign_tx, Blockchain, MempoolEntry, RawTxPayload,
    SignedTransaction, TxAdmissionError,
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
    let mut bc = Blockchain::new(&unique_path("temp_ttl"));
    bc.tx_ttl = 1;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 10_000, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    if let Some(mut entry) = bc.mempool.get_mut(&("alice".into(), 1)) {
        entry.timestamp_millis = 0;
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
    let mut bc = Blockchain::new(&unique_path("temp_fee_floor"));
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 0, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
}

#[test]
fn orphan_sweep_removes_missing_sender() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_orphan"));
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
    let mut bc = Blockchain::new(&unique_path("temp_orphan_ratio"));
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
fn drop_lock_poisoned_error_and_recovery() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_drop_poison"));
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
        assert_eq!(1, telemetry::TX_REJECTED_TOTAL.get());
    }
    bc.heal_mempool();
    assert_eq!(bc.drop_transaction("alice", 1), Ok(()));
}

#[test]
fn submit_lock_poisoned_error_and_recovery() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_submit_poison"));
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
        assert_eq!(1, telemetry::TX_REJECTED_TOTAL.get());
    }
    bc.heal_mempool();
    assert_eq!(bc.submit_transaction(tx), Ok(()));
}

#[test]
fn eviction_panic_rolls_back() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_evict_panic"));
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
    let mut bc = Blockchain::new(&unique_path("temp_admit_panic"));
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    for step in 0..2 {
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
    let mut bc = Blockchain::new(&unique_path("temp_cmp"));
    bc.tx_ttl = ttl;
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "alice", "bob", 1, 0, 2000, 1);
    let tx2 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 2);
    let tx3 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 3);
    let e1 = MempoolEntry {
        tx: tx1,
        timestamp_millis: 1,
    };
    let e2 = MempoolEntry {
        tx: tx2.clone(),
        timestamp_millis: 1,
    };
    let e3 = MempoolEntry {
        tx: tx3.clone(),
        timestamp_millis: 1,
    };
    let mut entries = vec![e3, e2.clone(), e1];
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    assert_eq!(entries[0].tx.payload.nonce, 1);
    let mut ids = [tx2.id(), tx3.id()];
    ids.sort();
    assert_eq!(entries[1].tx.id(), ids[0]);
    assert_eq!(entries[2].tx.id(), ids[1]);
}
