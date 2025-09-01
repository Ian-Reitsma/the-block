use ledger::utxo_account::{OutPoint, Utxo, UtxoAccountBridge};

#[test]
fn spent_utxo_updates_accounts() {
    let mut bridge = UtxoAccountBridge::new();
    bridge.accounts.credit("alice", 50);
    let op = OutPoint {
        txid: [1u8; 32],
        index: 0,
    };
    bridge.utxo.utxos.insert(
        op.clone(),
        Utxo {
            value: 50,
            owner: "alice".into(),
        },
    );
    bridge
        .apply_tx(&[op.clone()], &[("bob".into(), 50)])
        .unwrap();
    assert_eq!(bridge.accounts.balances.get("alice"), Some(&0));
    assert_eq!(bridge.accounts.balances.get("bob"), Some(&50));
    assert!(bridge.utxo.utxos.contains_key(&OutPoint {
        txid: blake3::hash(b"bridge_tx").into(),
        index: 0
    }));
}

#[test]
fn missing_utxo_is_atomic() {
    let mut bridge = UtxoAccountBridge::new();
    bridge.accounts.credit("alice", 10);
    let op = OutPoint {
        txid: [9u8; 32],
        index: 0,
    };
    let res = bridge.apply_tx(&[op.clone()], &[("bob".into(), 10)]);
    assert!(res.is_err());
    assert_eq!(bridge.accounts.balances.get("alice"), Some(&10));
    assert!(bridge.utxo.utxos.is_empty());
}
