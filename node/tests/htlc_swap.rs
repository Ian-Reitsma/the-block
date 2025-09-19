#![cfg(feature = "integration-tests")]
use sha3::{Digest, Sha3_256};
use the_block::vm::contracts::htlc::{HashAlgo, Htlc};

#[test]
fn successful_swap_redeem() {
    let preimage = b"secret";
    let mut h = Sha3_256::new();
    h.update(preimage);
    let hash = h.finalize().to_vec();
    let mut c = Htlc::new(hash, HashAlgo::Sha3, 100);
    assert!(c.redeem(preimage, 10));
}

#[test]
fn refund_path() {
    let preimage = b"secret";
    let mut h = Sha3_256::new();
    h.update(preimage);
    let hash = h.finalize().to_vec();
    let mut c = Htlc::new(hash, HashAlgo::Sha3, 50);
    assert!(c.refund(60));
}

#[test]
fn timeout_prevents_redeem() {
    let preimage = b"secret";
    let mut h = Sha3_256::new();
    h.update(preimage);
    let hash = h.finalize().to_vec();
    let mut c = Htlc::new(hash, HashAlgo::Sha3, 20);
    assert!(!c.redeem(preimage, 25));
    assert!(c.refund(25));
}
