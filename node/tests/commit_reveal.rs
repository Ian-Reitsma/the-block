#![cfg(feature = "integration-tests")]
use the_block::commit_reveal::{commit, verify};

#[test]
fn dilithium_commit_round_trip() {
    let salt = b"salt";
    let state = b"state";
    let (sig, nonce) = commit(salt, state, 42);
    if cfg!(feature = "pq-crypto") {
        assert!(sig.len() > 170, "need >170 for PQ signatures");
    } else {
        assert!(sig.len() <= 170);
    }
    assert!(verify(salt, state, &sig, nonce));
}
