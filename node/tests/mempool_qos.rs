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

    // Limit 1 per sender enforced.
    let second = state.reserve_sender("alice", 1).unwrap();
    let err = state
        .reserve_sender("alice", 1)
        .expect_err("should hit sender limit");
    assert!(matches!(err, TxAdmissionError::PendingLimitReached));

    // Dropping the reservation releases the slot.
    drop(second);
    assert!(state.reserve_sender("alice", 1).is_ok());
}
