#[path = "../compute_market/mod.rs"]
mod compute_market;

use the_block::compute_market::price_board::{backlog_adjusted_bid, reset};
use the_block::transaction::FeeLane;

#[test]
fn backlog_bid_generated_from_fuzz_data() {
    reset();
    let price: u64 = 512;
    let backlog: usize = 3;
    let mut data = Vec::new();
    data.extend_from_slice(&price.to_le_bytes());
    data.extend_from_slice(&(backlog as u64).to_le_bytes());

    compute_market::run(&data);

    let bid =
        backlog_adjusted_bid(FeeLane::Consumer, backlog).expect("backlog-adjusted bid available");
    assert!(bid > 0);
}
