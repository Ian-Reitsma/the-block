#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use foundation_serialization::binary;
use std::cmp::Ordering;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use the_block::{generate_keypair, mempool_cmp, sign_tx, Blockchain, MempoolEntry, RawTxPayload};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
    let _ = fs::remove_dir_all("chain_db");
}

fn build_entry(sk: &[u8], fee: u64, nonce: u64, ts: u64) -> MempoolEntry {
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee,
        pct_ct: 100,
        nonce,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).expect("valid key");
    let size = binary::encode(&tx).map(|b| b.len() as u64).unwrap_or(0);
    MempoolEntry {
        tx,
        timestamp_millis: ts,
        timestamp_ticks: ts,
        serialized_size: size,
    }
}

#[test]
fn comparator_orders_fee_then_expiry_then_hash() {
    init();
    let (sk, _pk) = generate_keypair();
    let ttl = 30;

    // Higher fee outranks lower fee
    let high_fee = build_entry(&sk, 2000, 1, 0);
    let low_fee = build_entry(&sk, 1000, 2, 0);
    assert_eq!(Ordering::Less, mempool_cmp(&high_fee, &low_fee, ttl));
    assert_eq!(Ordering::Greater, mempool_cmp(&low_fee, &high_fee, ttl));

    // Earlier expiry outranks later expiry when fees match
    let early = build_entry(&sk, 1000, 3, 0);
    let late = build_entry(&sk, 1000, 4, 10);
    assert_eq!(Ordering::Less, mempool_cmp(&early, &late, ttl));
    assert_eq!(Ordering::Greater, mempool_cmp(&late, &early, ttl));

    // When fee and expiry are equal, order by tx hash
    let a = build_entry(&sk, 1000, 5, 0);
    let b = build_entry(&sk, 1000, 6, 0);
    let expected = a.tx.id().cmp(&b.tx.id());
    assert_eq!(expected, mempool_cmp(&a, &b, ttl));
    assert_eq!(expected.reverse(), mempool_cmp(&b, &a, ttl));
}

#[test]
fn ordering_stable_after_heap_rebuild() {
    init();
    let (sk, _pk) = generate_keypair();
    let dir = temp_dir("heap_rebuild");
    let mut bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    bc.tx_ttl = 100;
    for acct in ["a", "b", "c", "d", "e"] {
        bc.add_account(acct.into(), 10_000, 10_000).unwrap();
    }

    let submit = |bc: &mut Blockchain, from: &str, fee: u64| {
        let payload = RawTxPayload {
            from_: from.into(),
            to: "sink".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.clone(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
    };

    submit(&mut bc, "a", 4_000);
    submit(&mut bc, "b", 3_000);
    submit(&mut bc, "c", 3_000);
    submit(&mut bc, "d", 2_000);
    submit(&mut bc, "e", 2_500);

    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    for (from, offset) in ["a", "b", "c", "d", "e"].iter().zip([0, 0, 10, 0, 0]) {
        if let Some(mut entry) = bc.mempool_consumer.get_mut(&(from.to_string(), 1)) {
            entry.timestamp_millis = base + offset;
            entry.timestamp_ticks = base + offset;
        }
    }

    let entry_a = bc
        .mempool_consumer
        .get(&(String::from("a"), 1))
        .map(|e| e.clone())
        .unwrap();
    let entry_e = bc
        .mempool_consumer
        .get(&(String::from("e"), 1))
        .map(|e| e.clone())
        .unwrap();
    let mut expected_entries = vec![entry_a, entry_e];
    expected_entries.sort_by(|a, b| mempool_cmp(a, b, bc.tx_ttl));
    let expected: Vec<[u8; 32]> = expected_entries.iter().map(|e| e.tx.id()).collect();

    bc.accounts.remove("b");
    bc.accounts.remove("c");
    bc.accounts.remove("d");
    bc.purge_expired();

    let mut after: Vec<MempoolEntry> = bc
        .mempool_consumer
        .iter()
        .map(|e| e.value().clone())
        .collect();
    after.sort_by(|a, b| mempool_cmp(a, b, bc.tx_ttl));
    let actual: Vec<[u8; 32]> = after.iter().map(|e| e.tx.id()).collect();

    assert_eq!(expected, actual);
}
