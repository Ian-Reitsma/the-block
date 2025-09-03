use the_block::commit_reveal::{commit, verify};

#[test]
fn dilithium_commit_round_trip() {
    let salt = b"salt";
    let state = b"state";
    let (sig, nonce) = commit(salt, state, 42);
    assert!(sig.len() <= 170);
    assert!(verify(salt, state, &sig, nonce));
}
