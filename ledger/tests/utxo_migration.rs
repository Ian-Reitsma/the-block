use ledger::utxo_account::migrate_accounts;
use std::collections::HashMap;

#[test]
fn migrate_accounts_generates_utxos() {
    let mut balances = HashMap::new();
    balances.insert("alice".to_string(), 50u64);
    let utxo = migrate_accounts(&balances);
    assert_eq!(utxo.utxos.len(), 1);
    let first = utxo.utxos.values().next().unwrap();
    assert_eq!(first.owner, "alice");
    assert_eq!(first.value, 50);
}
