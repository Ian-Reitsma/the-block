#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]
use sys::tempfile::tempdir;
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    fees::policy,
    generate_keypair, sign_tx,
    telemetry::{ADMISSION_MODE, INDUSTRIAL_REJECTED_TOTAL},
    Blockchain, FeeLane, RawTxPayload, TxAdmissionError,
};

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    fee: u64,
    nonce: u64,
) -> the_block::SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: 0,
        amount_industrial: 1,
        fee,
        pct: 0,
        nonce,
        memo: Vec::new(),
    };
    let mut tx = sign_tx(sk.to_vec(), payload).expect("sign");
    tx.lane = FeeLane::Industrial;
    tx
}

#[testkit::tb_serial]
fn rejects_industrial_when_consumer_fees_high() {
    let dir = tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("a".into(), 0, 2_000).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    bc.comfort_threshold_p90 = 10;
    for _ in 0..50 {
        policy::record_consumer_fee(20);
    }
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "a", "b", 1_000, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
    assert_eq!(
        ADMISSION_MODE
            .ensure_handle_for_label_values(&["tight"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get(),
        1
    );
    assert_eq!(
        INDUSTRIAL_REJECTED_TOTAL
            .ensure_handle_for_label_values(&["comfort_guard"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get(),
        1
    );
}
