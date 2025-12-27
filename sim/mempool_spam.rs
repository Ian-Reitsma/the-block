use the_block::transaction;
use the_block::{generate_keypair, Blockchain, FeeLane, RawTxPayload, TxAdmissionError};

fn main() {
    let (sk, _pk) = generate_keypair();
    let mut chain = Blockchain::default();
    chain.add_account("spam".into(), 1_000_000, 0).unwrap();
    chain.add_account("miner".into(), 0, 0).unwrap();

    chain.set_fee_floor_policy(32, 95);

    let mut accepted = 0;
    let mut rejected = 0;
    for nonce in 1..=96 {
        let fee = if nonce % 6 == 0 {
            5
        } else {
            150 + nonce as u64
        };
        let payload = RawTxPayload {
            from_: "spam".into(),
            to: "miner".into(),
            amount_consumer: 0,
            amount_industrial: 0,
            fee,
            pct: 100,
            nonce,
            memo: Vec::new(),
        };
        let mut tx = transaction::sign_tx(&sk, &payload).expect("sign");
        tx.tip = fee;
        tx.lane = FeeLane::Consumer;
        match chain.submit_transaction(tx) {
            Ok(_) => accepted += 1,
            Err(TxAdmissionError::FeeTooLow) => rejected += 1,
            Err(_) => {}
        }
    }

    let stats = chain.mempool_stats(FeeLane::Consumer);
    let (window, percentile) = chain.fee_floor_policy();
    println!(
        "window={} percentile={} floor={} mempool_size={} accepted={} rejected={}",
        window, percentile, stats.fee_floor, stats.size, accepted, rejected
    );
}
