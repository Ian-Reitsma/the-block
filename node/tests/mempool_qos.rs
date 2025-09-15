use the_block::mempool::scoring::FeeFloor;

#[test]
fn fee_floor_updates() {
    let mut ff = FeeFloor::new(4);
    ff.update(1);
    ff.update(2);
    ff.update(3);
    assert_eq!(ff.update(4), 3);
}
