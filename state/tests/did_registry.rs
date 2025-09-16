use state::did::{DidState, DidStateError};

#[test]
fn did_state_updates_monotonic_nonce() {
    let mut state = DidState::default();
    let hash1 = [1u8; 32];
    let hash2 = [2u8; 32];

    state.apply_update(1, hash1, 100).expect("first update");
    assert_eq!(state.nonce, 1);
    assert_eq!(state.hash, hash1);

    // replay should be rejected
    assert_eq!(
        state.apply_update(1, hash1, 110),
        Err(DidStateError::Replay)
    );

    state
        .apply_update(2, hash2, 120)
        .expect("second monotonic update");
    assert_eq!(state.nonce, 2);
    assert_eq!(state.hash, hash2);
    assert_eq!(state.updated_at, 120);
}
