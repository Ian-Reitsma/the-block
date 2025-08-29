use arbitrary::Unstructured;
use the_block::compute_market::price_board::{record_price, backlog_adjusted_bid, reset};

pub fn run(data: &[u8]) {
    let mut u = Unstructured::new(data);
    if let (Ok(price), Ok(backlog)) = (u.arbitrary::<u64>(), u.arbitrary::<usize>()) {
        reset();
        record_price(price);
        let _ = backlog_adjusted_bid(backlog);
    }
}
