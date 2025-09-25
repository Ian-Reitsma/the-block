use ledger::utxo_account::{AccountLedger, OutPoint, Utxo, UtxoAccountBridge, UtxoLedger};
use storage_engine::memory_engine::MemoryEngine;

#[test]
fn spent_utxo_updates_accounts() {
    let mut bridge = UtxoAccountBridge::new();
    bridge.accounts.deposit("alice", 50);
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
    bridge.accounts.deposit("alice", 10);
    let op = OutPoint {
        txid: [9u8; 32],
        index: 0,
    };
    let res = bridge.apply_tx(&[op.clone()], &[("bob".into(), 10)]);
    assert!(res.is_err());
    assert_eq!(bridge.accounts.balances.get("alice"), Some(&10));
    assert!(bridge.utxo.utxos.is_empty());
}

#[test]
fn account_ledger_persistence_roundtrip() {
    let engine = MemoryEngine::default();
    let mut ledger = AccountLedger::new();
    ledger.deposit("alice", 50);
    ledger.deposit("bob", 75);
    ledger
        .persist_to_engine(&engine, "settlement", "accounts")
        .expect("persist accounts");
    let loaded =
        AccountLedger::load_from_engine(&engine, "settlement", "accounts").expect("load accounts");
    assert_eq!(loaded.balances, ledger.balances);
}

#[test]
fn utxo_ledger_persistence_roundtrip() {
    let engine = MemoryEngine::default();
    let mut ledger = UtxoLedger::default();
    let op = OutPoint {
        txid: [1u8; 32],
        index: 0,
    };
    ledger.utxos.insert(
        op.clone(),
        Utxo {
            value: 42,
            owner: "carol".into(),
        },
    );
    ledger
        .persist_to_engine(&engine, "settlement", "utxos")
        .expect("persist utxos");
    let loaded = UtxoLedger::load_from_engine(&engine, "settlement", "utxos").expect("load utxos");
    assert_eq!(loaded.utxos.get(&op).unwrap().value, 42);
    assert_eq!(loaded.utxos.get(&op).unwrap().owner, "carol");
}
