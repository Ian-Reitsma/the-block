use ledger::address;
use node::transaction::{CrossShardEnvelope, RawTxPayload, SignedTransaction};

#[test]
fn routes_across_shards() {
    let mut tx = SignedTransaction::default();
    tx.payload = RawTxPayload {
        from_: "0001:alice".into(),
        to: "0002:bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct_ct: 0,
        nonce: 1,
        memo: vec![],
    };
    let env = CrossShardEnvelope::route(tx);
    assert_eq!(env.from_shard, 0x0001);
    assert_eq!(env.to_shard, 0x0002);
    assert_eq!(address::shard_id("0001:foo"), 1);
}
