use privacy::{Note, ShieldedMempool};

#[test]
fn duplicate_nullifier_rejected() {
    let note = Note {
        value: 5,
        rseed: [1u8; 32],
    };
    let nf = note.nullifier();
    let mut pool = ShieldedMempool::new();
    assert!(pool.check_and_insert(nf).is_ok());
    // Second insert of same nullifier must fail
    assert!(pool.check_and_insert(nf).is_err());
    assert_eq!(pool.pool_size(), 1);
}
