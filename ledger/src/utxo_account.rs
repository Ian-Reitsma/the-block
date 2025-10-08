use crypto_suite::hashing::blake3;
use std::collections::HashMap;
use std::convert::TryFrom;

use storage_engine::{KeyValue, StorageError, StorageResult};

#[derive(Clone, Debug)]
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
            Some(bytes) => decode_account_ledger(&bytes),
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
        let bytes = encode_account_ledger(&self.balances)?;
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub index: u32,
}

#[derive(Clone, Debug)]
pub struct Utxo {
    pub value: u64,
    pub owner: String,
}

#[derive(Default, Clone, Debug)]
pub struct UtxoLedger {
    pub utxos: HashMap<OutPoint, Utxo>,
}

impl UtxoLedger {
    pub fn load_from_engine<E: KeyValue>(engine: &E, cf: &str, key: &str) -> StorageResult<Self> {
        engine.ensure_cf(cf)?;
        match engine.get(cf, key.as_bytes())? {
            Some(bytes) => decode_utxo_ledger(&bytes),
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
        let bytes = encode_utxo_ledger(&self.utxos)?;
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

fn encode_account_ledger(balances: &HashMap<String, u64>) -> StorageResult<Vec<u8>> {
    let mut entries: Vec<(&String, &u64)> = balances.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut out = Vec::new();
    write_len(&mut out, entries.len())?;
    for (addr, value) in entries {
        write_string(&mut out, addr)?;
        write_u64(&mut out, *value);
    }
    Ok(out)
}

fn decode_account_ledger(bytes: &[u8]) -> StorageResult<AccountLedger> {
    let mut decoder = Decoder::new(bytes);
    let len = decoder.read_len()?;
    let mut balances = HashMap::with_capacity(len);
    for _ in 0..len {
        let addr = decoder.read_string()?;
        let value = decoder.read_u64()?;
        balances.insert(addr, value);
    }
    decoder.finish()?;
    Ok(AccountLedger { balances })
}

fn encode_utxo_ledger(utxos: &HashMap<OutPoint, Utxo>) -> StorageResult<Vec<u8>> {
    let mut entries: Vec<(&OutPoint, &Utxo)> = utxos.iter().collect();
    entries.sort_by(|a, b| {
        let order = a.0.txid.cmp(&b.0.txid);
        if order == std::cmp::Ordering::Equal {
            a.0.index.cmp(&b.0.index)
        } else {
            order
        }
    });

    let mut out = Vec::new();
    write_len(&mut out, entries.len())?;
    for (point, utxo) in entries {
        out.extend_from_slice(&point.txid);
        write_u32(&mut out, point.index);
        write_u64(&mut out, utxo.value);
        write_string(&mut out, &utxo.owner)?;
    }
    Ok(out)
}

fn decode_utxo_ledger(bytes: &[u8]) -> StorageResult<UtxoLedger> {
    let mut decoder = Decoder::new(bytes);
    let len = decoder.read_len()?;
    let mut utxos = HashMap::with_capacity(len);
    for _ in 0..len {
        let txid_bytes = decoder.read_exact(32)?;
        let mut txid = [0u8; 32];
        txid.copy_from_slice(txid_bytes);
        let index = decoder.read_u32()?;
        let value = decoder.read_u64()?;
        let owner = decoder.read_string()?;
        utxos.insert(OutPoint { txid, index }, Utxo { value, owner });
    }
    decoder.finish()?;
    Ok(UtxoLedger { utxos })
}

fn write_len(out: &mut Vec<u8>, len: usize) -> StorageResult<()> {
    let len = u32::try_from(len).map_err(|_| StorageError::backend("ledger too large"))?;
    write_u32(out, len);
    Ok(())
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) -> StorageResult<()> {
    let bytes = value.as_bytes();
    write_len(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

struct Decoder<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Decoder<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn remaining(&self, len: usize) -> StorageResult<()> {
        if self
            .position
            .checked_add(len)
            .map_or(true, |end| end > self.input.len())
        {
            return Err(StorageError::backend("corrupt ledger payload"));
        }
        Ok(())
    }

    fn read_exact(&mut self, len: usize) -> StorageResult<&'a [u8]> {
        self.remaining(len)?;
        let start = self.position;
        self.position += len;
        Ok(&self.input[start..start + len])
    }

    fn read_u32(&mut self) -> StorageResult<u32> {
        let bytes = self.read_exact(4)?;
        let mut buf = [0u8; 4];
        buf.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64(&mut self) -> StorageResult<u64> {
        let bytes = self.read_exact(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(buf))
    }

    fn read_len(&mut self) -> StorageResult<usize> {
        let value = self.read_u32()?;
        usize::try_from(value).map_err(|_| StorageError::backend("length overflow"))
    }

    fn read_string(&mut self) -> StorageResult<String> {
        let len = self.read_len()?;
        let bytes = self.read_exact(len)?;
        std::str::from_utf8(bytes)
            .map(|s| s.to_string())
            .map_err(|_| StorageError::backend("invalid string"))
    }

    fn finish(self) -> StorageResult<()> {
        if self.position == self.input.len() {
            Ok(())
        } else {
            Err(StorageError::backend("trailing bytes in ledger payload"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_outpoint(idx: u32) -> OutPoint {
        OutPoint {
            txid: [idx as u8; 32],
            index: idx,
        }
    }

    #[test]
    fn account_round_trip_preserves_balances() {
        let mut balances = HashMap::new();
        balances.insert("alice".to_string(), 42);
        balances.insert("🦀".to_string(), 7);

        let encoded = encode_account_ledger(&balances).expect("encode");
        let decoded = decode_account_ledger(&encoded).expect("decode");

        assert_eq!(balances.len(), decoded.balances.len());
        for (addr, amount) in balances {
            assert_eq!(Some(&amount), decoded.balances.get(&addr));
        }
    }

    #[test]
    fn utxo_round_trip_preserves_entries() {
        let mut utxos = HashMap::new();
        utxos.insert(
            sample_outpoint(0),
            Utxo {
                value: 50,
                owner: "carol".into(),
            },
        );
        utxos.insert(
            sample_outpoint(1),
            Utxo {
                value: 75,
                owner: "dan".into(),
            },
        );

        let encoded = encode_utxo_ledger(&utxos).expect("encode");
        let decoded = decode_utxo_ledger(&encoded).expect("decode");

        assert_eq!(utxos.len(), decoded.utxos.len());
        for (point, utxo) in utxos {
            let decoded_utxo = decoded.utxos.get(&point).expect("missing utxo");
            assert_eq!(utxo.value, decoded_utxo.value);
            assert_eq!(utxo.owner, decoded_utxo.owner);
        }
    }

    #[test]
    fn utxo_decode_rejects_truncated_payload() {
        let mut utxos = HashMap::new();
        utxos.insert(
            sample_outpoint(5),
            Utxo {
                value: 10,
                owner: "eve".into(),
            },
        );
        let mut bytes = encode_utxo_ledger(&utxos).expect("encode");
        bytes.pop(); // drop the final owner byte so the declared length no longer matches

        let err = decode_utxo_ledger(&bytes).expect_err("should fail");
        match err {
            StorageError::Backend(msg) => {
                assert!(msg.contains("corrupt"), "unexpected msg: {msg}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn account_decode_rejects_invalid_utf8() {
        let mut bytes = Vec::new();
        write_len(&mut bytes, 1).expect("write len");
        write_len(&mut bytes, 1).expect("write str len");
        bytes.push(0xFF); // invalid UTF-8 byte
        write_u64(&mut bytes, 0);

        let err = decode_account_ledger(&bytes).expect_err("should fail");
        match err {
            StorageError::Backend(msg) => {
                assert!(msg.contains("invalid string"), "unexpected msg: {msg}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
