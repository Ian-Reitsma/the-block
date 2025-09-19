#![cfg(feature = "integration-tests")]
use the_block::poh::Poh;

#[test]
fn poh_sequence_is_deterministic() {
    let mut a = Poh::new(b"seed");
    let h1 = a.tick();
    let h2 = a.record(b"tx");

    let mut b = Poh::new(b"seed");
    assert_eq!(h1, b.tick());
    assert_eq!(h2, b.record(b"tx"));
    assert_eq!(b.ticks(), 2);
}
