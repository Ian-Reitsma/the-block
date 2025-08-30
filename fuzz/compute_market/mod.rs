use arbitrary::Unstructured;
use the_block::compute_market::price_board::{record_price, backlog_adjusted_bid, reset};
use the_block::transaction::FeeLane;

pub fn run(data: &[u8]) {
    let mut u = Unstructured::new(data);
    if let (Ok(price), Ok(backlog)) = (u.arbitrary::<u64>(), u.arbitrary::<usize>()) {
        reset();
        record_price(FeeLane::Consumer, price);
        let _ = backlog_adjusted_bid(FeeLane::Consumer, backlog);
    }
}
