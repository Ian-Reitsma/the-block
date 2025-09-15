use std::collections::HashSet;

use ledger::address;
use the_block::{
    blockchain::process::{commit, StateDelta},
    Account, Blockchain, TokenBalance,
};

#[test]
fn shard_roots_persist() {
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
            industrial: 5,
        },
        nonce: 0,
        pending_consumer: 0,
        pending_industrial: 0,
        pending_nonce: 0,
        pending_nonces: HashSet::new(),
        sessions: Vec::new(),
    };

    let deltas = vec![
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
    ];
    commit(&mut bc, deltas).unwrap();

    let root1 = bc.get_shard_root(1).unwrap();
    let root2 = bc.get_shard_root(2).unwrap();

    drop(bc);
    let bc2 = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(bc2.get_shard_root(1).unwrap(), root1);
    assert_eq!(bc2.get_shard_root(2).unwrap(), root2);
}
