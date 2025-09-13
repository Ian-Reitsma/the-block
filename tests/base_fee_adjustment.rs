use the_block::{
    generate_keypair, sign_tx, Blockchain, FeeLane, RawTxPayload, SignedTransaction, TokenAmount,
};

fn build_tx(sk: &[u8], from: &str, to: &str, fee: u64, tip: u64, nonce: u64) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.into(),
        to: to.into(),
        amount_consumer: 0,
        amount_industrial: 0,
        fee,
        pct_ct: 100,
        nonce,
        memo: vec![],
    };
    let mut tx = sign_tx(sk, &payload).unwrap();
    tx.tip = tip;
    tx
}

#[test]
fn burns_base_fee_and_rewards_tip() {
    let (sk, _pk) = generate_keypair();
    let mut bc = Blockchain::default();
    bc.base_fee = 100;
    bc.block_reward_consumer = TokenAmount::new(0);
    bc.block_reward_industrial = TokenAmount::new(0);
    bc.add_account("a".into(), 10_000_000, 0).unwrap();
    bc.add_account("miner".into(), 0, 0).unwrap();
    let mut tx = build_tx(&sk, "a", "miner", 150, 50, 1);
    tx.lane = FeeLane::Consumer;
    bc.submit_transaction(tx).unwrap();
    let block = bc.mine_block("miner").unwrap();
    assert_eq!(block.base_fee, 100);
    let miner = bc.accounts.get("miner").unwrap();
    assert_eq!(miner.balance.consumer, 50);
}
