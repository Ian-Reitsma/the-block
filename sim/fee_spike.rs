//! Simple simulation of fee spikes to observe base fee reaction.
use the_block::{generate_keypair, sign_tx, Blockchain, FeeLane, RawTxPayload};

pub fn run_spike() {
    let (sk, _) = generate_keypair();
    let mut bc = Blockchain::default();
    bc.base_fee = 1;
    bc.add_account("a".into(), 10_000_000, 0).unwrap();
    bc.add_account("miner".into(), 0, 0).unwrap();
    for n in 1..=20 {
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "miner".into(),
            amount_consumer: 0,
            amount_industrial: 0,
            fee: bc.base_fee + 100,
            pct: 100,
            nonce: n,
            memo: vec![],
        };
        let mut tx = sign_tx(&sk, &payload).unwrap();
        tx.tip = 100;
        tx.lane = FeeLane::Consumer;
        let _ = bc.submit_transaction(tx);
    }
    let _ = bc.mine_block("miner");
    println!("base_fee={}", bc.base_fee);
}
