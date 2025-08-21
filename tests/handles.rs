use std::collections::HashSet;
use the_block::{Account, Blockchain, TokenBalance};

#[test]
fn register_and_resolve_handle() {
    let mut bc = Blockchain::default();
    bc.accounts.insert(
        "addr1".into(),
        Account {
            address: "addr1".into(),
            balance: TokenBalance {
                consumer: 0,
                industrial: 0,
            },
            nonce: 0,
            pending_consumer: 0,
            pending_industrial: 0,
            pending_nonce: 0,
            pending_nonces: HashSet::new(),
        },
    );
    assert!(bc.register_handle("@alice", "addr1"));
    assert_eq!(bc.resolve_handle("@alice"), Some("addr1".to_string()));
    assert!(!bc.register_handle("@alice", "addr1"));
}
