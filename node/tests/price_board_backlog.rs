#![cfg(feature = "integration-tests")]
use serial_test::serial;
use the_block::compute_market::price_board::{backlog_adjusted_bid, record_price, reset};
use the_block::transaction::FeeLane;

#[test]
#[serial]
fn backlog_adjusts_per_lane() {
    reset();
    for _ in 0..10 {
        record_price(FeeLane::Industrial, 100, 1.0);
    }
    let base = backlog_adjusted_bid(FeeLane::Industrial, 0).unwrap();
    assert_eq!(base, 100);
    let adj = backlog_adjusted_bid(FeeLane::Industrial, 25).unwrap();
    assert_eq!(adj, 125);
}
