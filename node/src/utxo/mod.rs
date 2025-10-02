#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
use std::collections::HashMap;

pub mod script;
pub use script::{execute, Op, Script};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub index: u32,
}

#[derive(Debug, Clone)]
pub struct TxIn {
    pub previous_output: OutPoint,
    pub script_sig: Script,
}

#[derive(Debug, Clone)]
pub struct TxOut {
    pub value: u64,
    pub script_pubkey: Script,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
}

#[derive(Default)]
pub struct Ledger {
    utxos: HashMap<OutPoint, TxOut>,
}

impl Ledger {
    pub fn apply_tx(&mut self, tx: &Transaction) -> Result<(), String> {
        for input in &tx.inputs {
            let prev = self
                .utxos
                .get(&input.previous_output)
                .ok_or("missing utxo")?;
            let mut stack = execute(&input.script_sig, &prev.script_pubkey)?;
            if stack.pop().unwrap_or_default() != 1u8 {
                // expecting OP_TRUE
                return Err("script failed".into());
            }
        }
        for input in &tx.inputs {
            self.utxos.remove(&input.previous_output);
        }
        let txid = tx.txid();
        for (i, out) in tx.outputs.iter().enumerate() {
            self.utxos.insert(
                OutPoint {
                    txid,
                    index: i as u32,
                },
                out.clone(),
            );
        }
        Ok(())
    }
}

impl Transaction {
    pub fn txid(&self) -> [u8; 32] {
        let mut h = Hasher::new();
        for input in &self.inputs {
            h.update(&input.previous_output.txid);
            h.update(&input.previous_output.index.to_le_bytes());
        }
        for output in &self.outputs {
            h.update(&output.value.to_le_bytes());
        }
        h.finalize().into()
    }
}
