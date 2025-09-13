use ledger::address;
use the_block::transaction::{CrossShardEnvelope, RawTxPayload};

#[test]
fn cross_shard_envelope_routes() {
    let from = address::encode(1, "alice");
    let to = address::encode(2, "bob");
    let payload = RawTxPayload {
        from_: from.clone(),
        to: to.clone(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct_ct: 0,
        nonce: 0,
        memo: Vec::new(),
    };
    let env = CrossShardEnvelope::new(address::shard_id(&from), address::shard_id(&to), payload);
    assert_eq!(env.origin, 1);
    assert_eq!(env.destination, 2);
    assert_eq!(address::shard_id(&env.payload.to), 2);
}
