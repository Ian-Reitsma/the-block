#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::util::temp::temp_dir;
use the_block::fee::{decompose, FeeError, MAX_FEE};
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

mod util;

#[test]
fn splits_pct_cases() {
    // 0% consumer -> full industrial
    assert_eq!(decompose(0, 10).unwrap(), (0, 10));
    // 37% consumer -> ceil rounding for consumer share
    let (consumer, industrial) = decompose(37, 10).unwrap();
    assert_eq!((consumer, industrial), (4, 6));
    assert_eq!(consumer + industrial, 10);
    // 100% consumer -> full consumer lane
    assert_eq!(decompose(100, 5).unwrap(), (5, 0));
}

#[test]
fn rejects_overflow_and_pct() {
    assert_eq!(decompose(101, 1).unwrap_err(), FeeError::InvalidSelector);
    assert_eq!(decompose(0, MAX_FEE + 1).unwrap_err(), FeeError::Overflow);
}

#[test]
fn admission_and_block_accounting() {
    let dir = temp_dir("fee_split_chain");
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.add_account("miner".into(), 0).unwrap();
    bc.add_account("alice".into(), 200).unwrap(); // Single BLOCK token (100+100)
    bc.add_account("bob".into(), 0).unwrap();
    bc.min_fee_per_byte_consumer = 0;
    bc.base_fee = 0;
    // Update dynamic pricing engine to allow zero fees for test
    bc.set_lane_base_fees(0, 0);
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 0,
        amount_industrial: 0,
        fee: 10,
        pct: 37,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    match bc.submit_transaction(tx) {
        Ok(_) => {}
        Err(e) => panic!("Transaction submission failed: {:?}", e),
    }
    let alice = bc.accounts.get("alice").unwrap();
    assert_eq!(alice.pending_amount, 10); // Total fee (4+6) in single BLOCK token
    bc.mine_block("miner").unwrap();
    let alice = bc.accounts.get("alice").unwrap();
    assert_eq!(alice.pending_amount, 0);
    assert_eq!(alice.balance.amount, 190); // 200 - 10
}
