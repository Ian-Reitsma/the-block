#![cfg(feature = "integration-tests")]
use the_block::mempool::scoring::FeeFloor;

#[test]
fn fee_floor_updates() {
    let mut ff = FeeFloor::new(4, 50);
    assert_eq!(ff.update(1), 1);
    assert_eq!(ff.update(2), 2);
    assert_eq!(ff.update(3), 2);
    assert_eq!(ff.update(4), 3);
    assert_eq!(ff.update(5), 4);
}
