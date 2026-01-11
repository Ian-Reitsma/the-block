#![cfg(feature = "integration-tests")]
// NOTE: This test was for legacy base_fee auto-adjustment which is no longer used.
// Lane-based dynamic pricing has replaced base_fee. This test is kept minimal to verify
// basic fee validation still works.

use the_block::{
    generate_keypair, sign_tx, Blockchain, FeeLane, RawTxPayload, SignedTransaction,
    TxAdmissionError,
};

fn build_tx(sk: &[u8], from: &str, to: &str, fee: u64, nonce: u64) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.into(),
        to: to.into(),
        amount_consumer: 0,
        amount_industrial: 0,
        fee,
        pct: 100,
        nonce,
        memo: vec![],
    };
    sign_tx(sk.to_vec(), payload).unwrap()
}

#[test]
fn rejects_underpriced_transactions() {
    let (sk, _pk) = generate_keypair();
    let mut bc = Blockchain::default();

    // Set minimum fee explicitly for testing
    bc.min_fee_per_byte_consumer = 100;
    bc.add_account("a".into(), 10_000_000).unwrap();
    bc.add_account("b".into(), 0).unwrap();

    // Transaction with sufficient fee should be accepted
    let mut high_fee_tx = build_tx(&sk, "a", "b", 100_000, 1);
    high_fee_tx.lane = FeeLane::Consumer;
    bc.submit_transaction(high_fee_tx).unwrap();

    // Transaction with fee below minimum should be rejected
    let mut low_fee_tx = build_tx(&sk, "a", "b", 1, 2);
    low_fee_tx.lane = FeeLane::Consumer;
    let err = bc.submit_transaction(low_fee_tx).unwrap_err();
    assert_eq!(err, TxAdmissionError::FeeTooLow);
}
