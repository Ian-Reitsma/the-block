#![cfg(feature = "integration-tests")]
use the_block::consensus::vdf::{evaluate, verify};

#[test]
fn pietrzak_vdf_round_trip() {
    let pre = b"preimage";
    let (out, proof) = evaluate(pre, 5);
    assert!(verify(pre, 5, &out, &proof));
}
