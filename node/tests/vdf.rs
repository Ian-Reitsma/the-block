#![cfg(feature = "integration-tests")]
use the_block::consensus::vdf::{evaluate, modulus_bits, verify};

#[test]
fn pietrzak_vdf_round_trip() {
    let pre = b"preimage";
    let (out, proof) = evaluate(pre, 5);
    assert!(verify(pre, 5, &out, &proof));
}

#[test]
fn modulus_size_is_configured() {
    assert_eq!(modulus_bits(), 256);
}
