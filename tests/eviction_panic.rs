#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction, TxAdmissionError,
};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
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
    amount: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.into(),
        to: to.into(),
        amount_consumer: amount,
        amount_industrial: amount,
        fee,
        fee_selector: 0,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("sign")
}

#[test]
fn eviction_panic_rolls_back() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("evict_panic");
    let _ = fs::remove_dir_all(&path);
    let mut bc = Blockchain::open(&path).unwrap();
    bc.max_mempool_size = 1;
    bc.add_account("a".into(), 10_000, 0).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    bc.mine_block("a").unwrap();

    #[cfg(feature = "telemetry")]
    {
        telemetry::LOCK_POISON_TOTAL.reset();
        telemetry::TX_REJECTED_TOTAL.reset();
    }

    let tx1 = build_signed_tx(&sk, "a", "b", 1, 1000, 1);
    bc.submit_transaction(tx1).unwrap();
    bc.panic_next_evict();
    let tx2 = build_signed_tx(&sk, "a", "b", 1, 1000, 2);
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bc.submit_transaction(tx2)));
    assert!(result.is_err());
    assert!(bc.mempool.is_empty());
    let acc = bc.accounts.get("a").unwrap();
    assert_eq!(acc.pending.nonce, 0);
    assert!(acc.pending.nonces.is_empty());
    let tx3 = build_signed_tx(&sk, "a", "b", 1, 1000, 3);
    assert_eq!(
        Err(TxAdmissionError::LockPoisoned),
        bc.submit_transaction(tx3)
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
}
