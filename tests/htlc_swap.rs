use ripemd::{Digest as RipemdDigest, Ripemd160};
use sha3::{Digest, Sha3_256};
use the_block::vm::contracts::htlc::{HashAlgo, Htlc};

fn hash_sha3(data: &[u8]) -> Vec<u8> {
    let mut h = Sha3_256::new();
    h.update(data);
    h.finalize().to_vec()
}

fn hash_ripemd(data: &[u8]) -> Vec<u8> {
    let mut h = Ripemd160::new();
    h.update(data);
    h.finalize().to_vec()
}

#[test]
fn successful_swap() {
    let preimage = b"s3cr3t";
    let hash = hash_sha3(preimage);
    let mut a = Htlc::new(hash.clone(), HashAlgo::Sha3, 10);
    let mut b = Htlc::new(hash, HashAlgo::Sha3, 10);
    assert!(a.redeem(preimage, 5));
    assert!(b.redeem(preimage, 5));
}

#[test]
fn refund_path() {
    let preimage = b"other";
    let hash = hash_ripemd(preimage);
    let mut h = Htlc::new(hash, HashAlgo::Ripemd160, 5);
    // counterparty never reveals, so refund after timeout
    assert!(!h.redeem(preimage, 6));
    assert!(h.refund(6));
}

#[test]
fn timeout_prevents_late_redeem() {
    let preimage = b"late";
    let hash = hash_sha3(preimage);
    let mut h = Htlc::new(hash, HashAlgo::Sha3, 3);
    assert!(!h.redeem(preimage, 4));
    // once timeout passes, refund succeeds
    assert!(h.refund(4));
}
