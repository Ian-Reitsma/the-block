use foundation_fuzz::Unstructured;
use the_block::compute_market::price_board::{backlog_adjusted_bid, record_price, reset};
use the_block::transaction::FeeLane;

pub fn run(data: &[u8]) {
    let mut u = Unstructured::new(data);
    if let (Ok(price), Ok(backlog)) = (u.arbitrary::<u64>(), u.arbitrary::<usize>()) {
        reset();
        record_price(FeeLane::Consumer, price, 1.0);
        let _ = backlog_adjusted_bid(FeeLane::Consumer, backlog);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_records_backlog_bid_from_fuzz_data() {
        reset();
        let price: u64 = 250;
        let backlog: usize = 4;
        let mut data = Vec::new();
        data.extend_from_slice(&price.to_le_bytes());
        data.extend_from_slice(&(backlog as u64).to_le_bytes());

        run(&data);

        let adjusted = backlog_adjusted_bid(FeeLane::Consumer, backlog)
            .expect("backlog-adjusted bid should be available");
        assert!(adjusted > 0);
    }
}
