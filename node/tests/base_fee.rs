#![cfg(feature = "integration-tests")]
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
fn adjusts_base_fee_and_rejects_underpriced() {
    let (sk, _pk) = generate_keypair();
    let mut bc = Blockchain::default();
    bc.base_fee = 100;
    bc.add_account("a".into(), 10_000_000, 0).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    bc.max_pending_per_account = 100;
    // submit 20 txs with high fee to drive base fee up
    for n in 1..=20 {
        let mut tx = build_tx(&sk, "a", "b", 100_000, n);
        tx.lane = FeeLane::Consumer;
        bc.submit_transaction(tx).unwrap();
    }
    bc.mine_block("miner").unwrap();
    assert!(bc.base_fee > 100);
    // tx below base fee should be rejected
    let mut low = build_tx(&sk, "a", "b", bc.base_fee - 1, 21);
    low.lane = FeeLane::Consumer;
    let err = bc.submit_transaction(low).unwrap_err();
    assert_eq!(err, TxAdmissionError::FeeTooLow);
}
