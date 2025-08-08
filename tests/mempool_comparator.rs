#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cmp::Ordering;
use std::fs;
use the_block::{generate_keypair, mempool_cmp, sign_tx, MempoolEntry, RawTxPayload};

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
        fee_selector: 0,
        nonce,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).expect("valid key");
    MempoolEntry {
        tx,
        timestamp_millis: ts,
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

