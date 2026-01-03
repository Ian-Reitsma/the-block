#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::needless_range_loop)]

use foundation_serialization::binary;
use std::fs;
use std::sync::Once;
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, mempool_cmp, sign_tx, Blockchain, MempoolEntry, RawTxPayload,
    SignedTransaction, TxAdmissionError,
};

mod util;
use util::temp::temp_dir;

static PY_INIT: Once = Once::new();

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    PY_INIT.call_once(|| {});
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
    // NOTE: Post single-token migration, amount_consumer/amount_industrial represent
    // LANE routing (not separate token types). Mining only credits consumer balance.
    // Tests should use consumer lane only (industrial=0) unless explicitly testing industrial lane.
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

#[testkit::tb_serial]
fn replacement_rejected() {
    init();
    let dir = temp_dir("temp_replace");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.min_fee_per_byte_consumer = 0;
    bc.min_fee_per_byte_industrial = 0;
    bc.add_account("miner".into(), 0).unwrap();
    bc.add_account("alice".into(), 0).unwrap();
    bc.mine_block("miner").unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "miner", "alice", 1, 0, 1000, 1); // industrial=0 (single token via consumer lane)
    bc.submit_transaction(tx.clone()).unwrap();
    let res = bc.submit_transaction(tx);
    assert!(matches!(res, Err(TxAdmissionError::Duplicate)));
}

#[testkit::tb_serial]
fn eviction_via_drop_transaction() {
    init();
    let dir = temp_dir("temp_evict");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.min_fee_per_byte_consumer = 0;
    bc.min_fee_per_byte_industrial = 0;
    bc.max_mempool_size_consumer = 1;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 10_000).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_b, _pk_b) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    let tx2 = build_signed_tx(&sk_b, "bob", "alice", 1, 0, 2000, 1);
    bc.submit_transaction(tx2.clone()).unwrap();
    assert!(bc.mempool_consumer.contains_key(&("bob".to_string(), 1)));
    bc.drop_transaction("bob", 1).unwrap();
    let tx3 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 2500, 1);
    bc.submit_transaction(tx3).unwrap();
}

#[testkit::tb_serial]
fn ttl_expiry_purges_and_counts() {
    init();
    let dir = temp_dir("temp_ttl");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = 1;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 10_000).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    if let Some(mut entry) = bc.mempool_consumer.get_mut(&("alice".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    #[cfg(feature = "telemetry")]
    telemetry::TTL_DROP_TOTAL.reset();
    let dropped = bc.purge_expired();
    assert_eq!(1, dropped);
    assert!(bc.mempool_consumer.is_empty());
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::TTL_DROP_TOTAL.value());
}

#[testkit::tb_serial]
fn fee_floor_enforced() {
    init();
    let dir = temp_dir("temp_fee_floor");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 0, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
}

#[testkit::tb_serial]
fn orphan_sweep_removes_missing_sender() {
    init();
    let dir = temp_dir("temp_orphan");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx).unwrap();
    bc.accounts.remove("alice");
    #[cfg(feature = "telemetry")]
    telemetry::ORPHAN_SWEEP_TOTAL.reset();
    let _ = bc.purge_expired();
    assert!(bc.mempool_consumer.is_empty());
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::ORPHAN_SWEEP_TOTAL.value());
}

#[testkit::tb_serial]
fn orphan_ratio_triggers_rebuild() {
    init();
    let dir = temp_dir("temp_orphan_ratio");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk, _pk) = generate_keypair();
    for n in 1..=3 {
        let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, n);
        bc.submit_transaction(tx).unwrap();
    }
    bc.accounts.remove("alice");
    let _ = bc.purge_expired();
    assert_eq!(bc.mempool_consumer.len(), 0);
    assert_eq!(bc.orphan_count(), 0);
}

#[testkit::tb_serial]
fn heap_orphan_stress_triggers_rebuild_and_orders() {
    init();
    let dir = temp_dir("temp_heap_orphan");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = 16;
    bc.add_account("sink".into(), 0).unwrap();
    let mut keys = Vec::new();
    for i in 0..7 {
        let name = format!("acct{i}");
        bc.add_account(name.clone(), 10_000).unwrap();
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
    assert_eq!(bc.mempool_consumer.len(), 3);
    assert_eq!(bc.orphan_count(), 0);
    #[cfg(feature = "telemetry")]
    assert_eq!(1, telemetry::ORPHAN_SWEEP_TOTAL.value());
    let ttl = bc.tx_ttl;
    let mut entries = Vec::new();
    bc.mempool_consumer
        .for_each(|_key, value| entries.push(value.clone()));
    entries.sort_by(|a, b| mempool_cmp(a, b, ttl));
    for w in entries.windows(2) {
        assert!(mempool_cmp(&w[0], &w[1], ttl) != std::cmp::Ordering::Greater);
    }
}

#[testkit::tb_serial]
fn orphan_drop_decrements_counter() {
    init();
    let dir = temp_dir("temp_orphan_drop");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("carol".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
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

#[testkit::tb_serial]
fn ttl_purge_drops_orphan_and_decrements_counter() {
    init();
    let dir = temp_dir("temp_orphan_ttl");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = 1;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("carol".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk_a, _pk_a) = generate_keypair();
    let (sk_c, _pk_c) = generate_keypair();
    let tx1 = build_signed_tx(&sk_a, "alice", "bob", 1, 0, 1000, 1);
    let tx2 = build_signed_tx(&sk_c, "carol", "bob", 1, 0, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    bc.submit_transaction(tx2).unwrap();
    bc.accounts.remove("alice");
    let _ = bc.purge_expired();
    assert_eq!(bc.orphan_count(), 1);
    if let Some(mut entry) = bc.mempool_consumer.get_mut(&("alice".into(), 1)) {
        entry.timestamp_millis = 0;
        entry.timestamp_ticks = 0;
    }
    let dropped = bc.purge_expired();
    assert_eq!(dropped, 1);
    assert_eq!(bc.orphan_count(), 0);
    assert_eq!(bc.mempool_consumer.len(), 1);
}

#[testkit::tb_serial]
fn drop_lock_poisoned_error_and_recovery() {
    init();
    let dir = temp_dir("temp_drop_poison");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
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
        assert_eq!(1, telemetry::LOCK_POISON_TOTAL.value());
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .ensure_handle_for_label_values(&["lock_poison"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
        );
    }
    bc.heal_mempool();
    assert_eq!(bc.drop_transaction("alice", 1), Ok(()));
}

#[testkit::tb_serial]
fn submit_lock_poisoned_error_and_recovery() {
    init();
    let dir = temp_dir("temp_submit_poison");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
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
        assert_eq!(1, telemetry::LOCK_POISON_TOTAL.value());
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .ensure_handle_for_label_values(&["lock_poison"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
        );
    }
    bc.heal_mempool();
    assert_eq!(bc.submit_transaction(tx), Ok(()));
}

#[testkit::tb_serial]
fn eviction_panic_rolls_back() {
    init();
    let dir = temp_dir("temp_evict_panic");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.max_mempool_size_consumer = 1;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 10_000).unwrap();
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
    assert_eq!(bc.mempool_consumer.len(), 0);
    bc.submit_transaction(tx2).unwrap();
    assert_eq!(bc.mempool_consumer.len(), 1);
}

#[testkit::tb_serial]
fn admission_panic_rolls_back() {
    init();
    let dir = temp_dir("temp_admit_panic");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
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
        assert!(bc.mempool_consumer.is_empty());
        let acc = bc.accounts.get("alice").unwrap();
        assert_eq!(acc.pending_amount, 0);
        assert_eq!(acc.pending_nonce, 0);
        assert!(acc.pending_nonces.is_empty());
    }
}

#[testkit::tb_serial]
fn comparator_orders_by_fee_expiry_hash() {
    init();
    let ttl = 10;
    let dir = temp_dir("temp_cmp");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.tx_ttl = ttl;
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "alice", "bob", 1, 0, 2000, 1);
    let tx2 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 2);
    let tx3 = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 3);
    let size1 = binary::encode(&tx1).map(|b| b.len() as u64).unwrap_or(0);
    let size2 = binary::encode(&tx2).map(|b| b.len() as u64).unwrap_or(0);
    let size3 = binary::encode(&tx3).map(|b| b.len() as u64).unwrap_or(0);
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
