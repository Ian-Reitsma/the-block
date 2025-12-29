#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction, TxAdmissionError,
};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {});
}

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransaction {
    // NOTE: Post single-token migration, amount_consumer/amount_industrial represent
    // LANE routing (not separate token types). Single BLOCK token routed via consumer lane.
    let payload = RawTxPayload {
        from_: from.into(),
        to: to.into(),
        amount_consumer: amount,
        amount_industrial: 0,  // Single BLOCK token via consumer lane only
        fee,
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("sign")
}

#[test]
fn eviction_panic_rolls_back() {
    init();
    let (sk, _pk) = generate_keypair();
    let dir = temp_dir("evict_panic");
    let mut bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    bc.max_mempool_size_consumer = 1;
    bc.add_account("a".into(), 100_000).unwrap();  // Single BLOCK token
    bc.add_account("b".into(), 0).unwrap();
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
    assert!(bc.mempool_consumer.is_empty());
    let acc = bc.accounts.get("a").unwrap();
    assert_eq!(acc.pending_nonce, 0);
    assert_eq!(acc.pending_amount, 0);
    assert!(acc.pending_nonces.is_empty());
    let tx3 = build_signed_tx(&sk, "a", "b", 1, 1000, 3);
    #[cfg(feature = "telemetry")]
    {
        let before_lp = telemetry::LOCK_POISON_TOTAL.value();
        let before_rej = telemetry::TX_REJECTED_TOTAL
            .ensure_handle_for_label_values(&["lock_poison"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        assert_eq!(before_lp, before_rej);
        assert_eq!(
            Err(TxAdmissionError::LockPoisoned),
            bc.submit_transaction(tx3)
        );
        let after_lp = telemetry::LOCK_POISON_TOTAL.value();
        let after_rej = telemetry::TX_REJECTED_TOTAL
            .ensure_handle_for_label_values(&["lock_poison"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();
        assert_eq!(before_lp + 1, after_lp);
        assert_eq!(before_rej + 1, after_rej);
        assert_eq!(after_lp, after_rej);
    }
    #[cfg(not(feature = "telemetry"))]
    assert_eq!(
        Err(TxAdmissionError::LockPoisoned),
        bc.submit_transaction(tx3)
    );
}
