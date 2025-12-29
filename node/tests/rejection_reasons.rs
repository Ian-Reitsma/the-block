#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use std::fs;
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction, TxAdmissionError,
};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
}

#[allow(clippy::too_many_arguments)]
fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    consumer: u64,
    industrial: u64,
    fee: u64,
    nonce: u64,
    selector: u8,
) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: consumer,
        amount_industrial: industrial,
        fee,
        pct: selector,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid key")
}

#[testkit::tb_serial]
fn invalid_selector_rejects_and_counts() {
    init();
    let dir = temp_dir("temp_invalid_selector");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1, 255);
    #[cfg(feature = "telemetry")]
    {
        telemetry::TX_REJECTED_TOTAL.reset();
        telemetry::INVALID_SELECTOR_REJECT_TOTAL.reset();
    }
    assert_eq!(
        bc.submit_transaction(tx),
        Err(TxAdmissionError::InvalidSelector)
    );
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .ensure_handle_for_label_values(&["invalid_selector"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
        );
        assert_eq!(1, telemetry::INVALID_SELECTOR_REJECT_TOTAL.value());
    }
}

#[testkit::tb_serial]
fn balance_overflow_rejects_and_counts() {
    init();
    let dir = temp_dir("temp_balance_overflow");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), u64::MAX).unwrap();
    bc.add_account("bob".into(), 0).unwrap();
    // create pending reservation near limit to force overflow
    {
        let acc = bc.accounts.get_mut("alice").unwrap();
        acc.pending_amount = u64::MAX - 1;
    }
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1, 1, 100);
    #[cfg(feature = "telemetry")]
    {
        telemetry::TX_REJECTED_TOTAL.reset();
        telemetry::BALANCE_OVERFLOW_REJECT_TOTAL.reset();
    }
    assert_eq!(
        bc.submit_transaction(tx),
        Err(TxAdmissionError::BalanceOverflow)
    );
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .ensure_handle_for_label_values(&["balance_overflow"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
        );
        assert_eq!(1, telemetry::BALANCE_OVERFLOW_REJECT_TOTAL.value());
    }
}

#[testkit::tb_serial]
fn drop_not_found_rejects_and_counts() {
    init();
    let dir = temp_dir("temp_drop_not_found");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("alice".into(), 10_000).unwrap();
    #[cfg(feature = "telemetry")]
    {
        telemetry::TX_REJECTED_TOTAL.reset();
        telemetry::DROP_NOT_FOUND_TOTAL.reset();
    }
    assert_eq!(
        bc.drop_transaction("alice", 1),
        Err(TxAdmissionError::NotFound)
    );
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(
            1,
            telemetry::TX_REJECTED_TOTAL
                .ensure_handle_for_label_values(&["not_found"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get()
        );
        assert_eq!(1, telemetry::DROP_NOT_FOUND_TOTAL.value());
    }
}
