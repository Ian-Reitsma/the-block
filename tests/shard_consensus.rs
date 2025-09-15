use std::collections::HashSet;

use ledger::address;
use node::{
    blockchain::process::{commit, StateDelta},
    transaction::{CrossShardEnvelope, RawTxPayload, SignedTransaction},
    Account, Blockchain, TokenBalance,
};

#[test]
fn cross_shard_reorg_updates_roots() {
    // Craft envelopes across shards
    let mut tx1 = SignedTransaction::default();
    tx1.payload = RawTxPayload {
        from_: "0001:alice".into(),
        to: "0002:bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct_ct: 0,
        nonce: 1,
        memo: vec![],
    };
    let env1 = CrossShardEnvelope::route(tx1.clone());
    assert_eq!(env1.from_shard, 0x0001);
    assert_eq!(env1.to_shard, 0x0002);

    let mut tx2 = SignedTransaction::default();
    tx2.payload = RawTxPayload {
        from_: "0002:bob".into(),
        to: "0001:alice".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        pct_ct: 0,
        nonce: 1,
        memo: vec![],
    };
    let env2 = CrossShardEnvelope::route(tx2.clone());
    assert_eq!(env2.from_shard, 0x0002);
    assert_eq!(env2.to_shard, 0x0001);

    // Initialise blockchain with two shards
    let dir = tempfile::tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    let addr1 = address::encode(1, "alice");
    let addr2 = address::encode(2, "bob");
    let acc1 = Account {
        address: addr1.clone(),
        balance: TokenBalance {
            consumer: 10,
            industrial: 0,
        },
        nonce: 0,
        pending_consumer: 0,
        pending_industrial: 0,
        pending_nonce: 0,
        pending_nonces: HashSet::new(),
        sessions: Vec::new(),
    };
    let acc2 = Account {
        address: addr2.clone(),
        balance: TokenBalance {
            consumer: 0,
            industrial: 0,
        },
        nonce: 0,
        pending_consumer: 0,
        pending_industrial: 0,
        pending_nonce: 0,
        pending_nonces: HashSet::new(),
        sessions: Vec::new(),
    };
    commit(
        &mut bc,
        vec![
            StateDelta {
                address: addr1.clone(),
                account: acc1.clone(),
                shard: address::shard_id(&addr1),
            },
            StateDelta {
                address: addr2.clone(),
                account: acc2.clone(),
                shard: address::shard_id(&addr2),
            },
        ],
    )
    .unwrap();
    let base_root1 = bc.get_shard_root(1).unwrap();
    let base_root2 = bc.get_shard_root(2).unwrap();

    // Block A: alice -> bob
    let mut acc1_a = acc1.clone();
    acc1_a.balance.consumer -= 1;
    acc1_a.nonce = 1;
    let mut acc2_a = acc2.clone();
    acc2_a.balance.consumer += 1;
    commit(
        &mut bc,
        vec![
            StateDelta {
                address: addr1.clone(),
                account: acc1_a.clone(),
                shard: address::shard_id(&addr1),
            },
            StateDelta {
                address: addr2.clone(),
                account: acc2_a.clone(),
                shard: address::shard_id(&addr2),
            },
        ],
    )
    .unwrap();
    let root1_a = bc.get_shard_root(1).unwrap();
    let root2_a = bc.get_shard_root(2).unwrap();

    // Re-org: roll back to base state
    bc.accounts.insert(addr1.clone(), acc1.clone());
    bc.accounts.insert(addr2.clone(), acc2.clone());
    bc.shard_roots.insert(1, base_root1);
    bc.shard_roots.insert(2, base_root2);
    assert_eq!(bc.get_shard_root(1).unwrap(), base_root1);
    assert_eq!(bc.get_shard_root(2).unwrap(), base_root2);

    // Block B: bob -> alice
    let mut acc1_b = acc1.clone();
    acc1_b.balance.consumer += 1;
    let mut acc2_b = acc2.clone();
    acc2_b.balance.consumer -= 1;
    acc2_b.nonce = 1;
    commit(
        &mut bc,
        vec![
            StateDelta {
                address: addr1.clone(),
                account: acc1_b.clone(),
                shard: address::shard_id(&addr1),
            },
            StateDelta {
                address: addr2.clone(),
                account: acc2_b.clone(),
                shard: address::shard_id(&addr2),
            },
        ],
    )
    .unwrap();
    let root1_b = bc.get_shard_root(1).unwrap();
    let root2_b = bc.get_shard_root(2).unwrap();

    assert_ne!(root1_a, root1_b);
    assert_ne!(root2_a, root2_b);
}
