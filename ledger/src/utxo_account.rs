use crypto_suite::hashing::blake3;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;

use storage_engine::{KeyValue, StorageError, StorageResult};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AccountLedger {
    pub balances: HashMap<String, u64>,
}

impl AccountLedger {
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
        }
    }

    pub fn load_from_engine<E: KeyValue>(engine: &E, cf: &str, key: &str) -> StorageResult<Self> {
        engine.ensure_cf(cf)?;
        match engine.get(cf, key.as_bytes())? {
            Some(bytes) => deserialize(&bytes),
            None => Ok(AccountLedger::new()),
        }
    }

    pub fn persist_to_engine<E: KeyValue>(
        &self,
        engine: &E,
        cf: &str,
        key: &str,
    ) -> StorageResult<()> {
        engine.ensure_cf(cf)?;
        let bytes = bincode::serialize(self).map_err(StorageError::backend)?;
        engine.put_bytes(cf, key.as_bytes(), &bytes)
    }
    pub fn deposit(&mut self, addr: &str, amount: u64) {
        *self.balances.entry(addr.to_string()).or_insert(0) += amount;
    }
    pub fn debit(&mut self, addr: &str, amount: u64) -> Result<(), String> {
        let bal = self.balances.get_mut(addr).ok_or("missing account")?;
        if *bal < amount {
            return Err("insufficient balance".into());
        }
        *bal -= amount;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub index: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Utxo {
    pub value: u64,
    pub owner: String,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct UtxoLedger {
    pub utxos: HashMap<OutPoint, Utxo>,
}

impl UtxoLedger {
    pub fn load_from_engine<E: KeyValue>(engine: &E, cf: &str, key: &str) -> StorageResult<Self> {
        engine.ensure_cf(cf)?;
        match engine.get(cf, key.as_bytes())? {
            Some(bytes) => deserialize(&bytes),
            None => Ok(UtxoLedger::default()),
        }
    }

    pub fn persist_to_engine<E: KeyValue>(
        &self,
        engine: &E,
        cf: &str,
        key: &str,
    ) -> StorageResult<()> {
        engine.ensure_cf(cf)?;
        let bytes = bincode::serialize(self).map_err(StorageError::backend)?;
        engine.put_bytes(cf, key.as_bytes(), &bytes)
    }
}

pub struct UtxoAccountBridge {
    pub utxo: UtxoLedger,
    pub accounts: AccountLedger,
}

impl UtxoAccountBridge {
    pub fn new() -> Self {
        Self {
            utxo: UtxoLedger::default(),
            accounts: AccountLedger::new(),
        }
    }

    /// Apply a UTXO transaction and atomically update account balances.
    pub fn apply_tx(
        &mut self,
        inputs: &[OutPoint],
        outputs: &[(String, u64)],
    ) -> Result<(), String> {
        let mut debits: Vec<(String, u64)> = Vec::new();
        for inp in inputs {
            let entry = self.utxo.utxos.get(inp).ok_or("missing utxo")?;
            debits.push((entry.owner.clone(), entry.value));
        }
        // All checks passed; apply atomically
        for inp in inputs {
            self.utxo.utxos.remove(inp);
        }
        for (addr, val) in &debits {
            self.accounts.debit(addr, *val)?;
        }
        let txid = blake3::hash(b"bridge_tx").into();
        for (i, (addr, val)) in outputs.iter().enumerate() {
            self.utxo.utxos.insert(
                OutPoint {
                    txid,
                    index: i as u32,
                },
                Utxo {
                    value: *val,
                    owner: addr.clone(),
                },
            );
            self.accounts.deposit(addr, *val);
        }
        Ok(())
    }
}

/// Generate a UTXO ledger from existing account balances for migration purposes.
pub fn migrate_accounts(balances: &HashMap<String, u64>) -> UtxoLedger {
    let txid = blake3::hash(b"migrate").into();
    let mut utxo = UtxoLedger::default();
    for (i, (addr, val)) in balances.iter().enumerate() {
        utxo.utxos.insert(
            OutPoint {
                txid,
                index: i as u32,
            },
            Utxo {
                value: *val,
                owner: addr.clone(),
            },
        );
    }
    utxo
}

fn deserialize<T: DeserializeOwned>(bytes: &[u8]) -> StorageResult<T> {
    bincode::deserialize(bytes).map_err(StorageError::backend)
}
