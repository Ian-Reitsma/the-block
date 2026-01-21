#![cfg(feature = "integration-tests")]

use the_block::mempool::admission::AdmissionState;
use the_block::TxAdmissionError;

#[test]
fn admission_enforces_sender_limits_and_tracks_fee_floor() {
    let mut state = AdmissionState::new(4, 50, "consumer");

    // First reservation succeeds and updates fee floor on commit.
    {
        let reservation = state.reserve_sender("alice", 1).unwrap();
        reservation.commit(12);
    }
    assert_eq!(state.floor(), 12);

    // Limit 1 per sender enforced (reservation remains held after commit).
    let err = match state.reserve_sender("alice", 1) {
        Ok(_) => panic!("should hit sender limit"),
        Err(err) => err,
    };
    assert!(matches!(err, TxAdmissionError::PendingLimitReached));

    // Releasing the slot allows new reservations.
    state.release_sender("alice");
    assert!(state.reserve_sender("alice", 1).is_ok());
}
