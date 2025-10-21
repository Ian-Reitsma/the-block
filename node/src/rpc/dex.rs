#![forbid(unsafe_code)]

use crate::dex::{check_liquidity_rules, storage::EscrowState, DexStore, TrustLedger};
use concurrency::Lazy;
use dex::escrow::{HashAlgo, PaymentProof};
use foundation_serialization::json::{Map, Number, Value};
use std::sync::Mutex;

pub static STORE: Lazy<Mutex<DexStore>> = Lazy::new(|| Mutex::new(DexStore::open("dex_db")));
pub static LEDGER: Lazy<Mutex<TrustLedger>> = Lazy::new(|| Mutex::new(TrustLedger::default()));

fn hash_algo_str(algo: HashAlgo) -> &'static str {
    match algo {
        HashAlgo::Blake3 => "Blake3",
        HashAlgo::Sha3 => "Sha3",
    }
}

fn bytes32_to_value(bytes: &[u8; 32]) -> Value {
    Value::Array(
        bytes
            .iter()
            .map(|b| Value::Number(Number::from(*b)))
            .collect(),
    )
}

fn payment_proof_to_value(proof: &PaymentProof) -> Value {
    let mut map = Map::new();
    map.insert("leaf".to_string(), bytes32_to_value(&proof.leaf));
    let path: Vec<Value> = proof
        .path
        .iter()
        .map(|segment| bytes32_to_value(segment))
        .collect();
    map.insert("path".to_string(), Value::Array(path));
    map.insert(
        "algo".to_string(),
        Value::String(hash_algo_str(proof.algo).to_string()),
    );
    Value::Object(map)
}

pub fn escrow_status(id: u64) -> Value {
    let store = STORE.lock().unwrap();
    let state: EscrowState = store.load_escrow_state();
    if let Some(entry) = state.escrow.status(id) {
        let mut proofs = Vec::new();
        for (idx, amount) in entry.payments.iter().enumerate() {
            if let Some(p) = state.escrow.proof(id, idx) {
                let mut proof_obj = Map::new();
                proof_obj.insert("amount".to_string(), Value::Number(Number::from(*amount)));
                proof_obj.insert("proof".to_string(), payment_proof_to_value(&p));
                proofs.push(Value::Object(proof_obj));
            }
        }
        let mut obj = Map::new();
        obj.insert("from".to_string(), Value::String(entry.from.clone()));
        obj.insert("to".to_string(), Value::String(entry.to.clone()));
        obj.insert(
            "total".to_string(),
            Value::Number(Number::from(entry.total)),
        );
        obj.insert(
            "released".to_string(),
            Value::Number(Number::from(entry.released)),
        );
        obj.insert(
            "outstanding".to_string(),
            Value::Number(Number::from(entry.total - entry.released)),
        );
        obj.insert("proofs".to_string(), Value::Array(proofs));
        Value::Object(obj)
    } else {
        let mut obj = Map::new();
        obj.insert("error".to_string(), Value::String("not_found".to_string()));
        Value::Object(obj)
    }
}

pub fn escrow_release(id: u64, amount: u64) -> Result<Value, &'static str> {
    let mut store = STORE.lock().unwrap();
    let mut state = store.load_escrow_state();
    let (buy, sell, _qty, locked_at) = state.locks.get(&id).cloned().ok_or("not_found")?;
    check_liquidity_rules(locked_at)?;
    let proof = state.escrow.release(id, amount).ok_or("invalid_release")?;
    if let Some(entry) = state.escrow.status(id) {
        if entry.released == entry.total {
            state.locks.remove(&id);
        }
    } else {
        state.locks.remove(&id);
    }
    store.log_trade(&(buy.clone(), sell.clone(), amount), &proof);
    store.save_escrow_state(&state);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::DEX_ESCROW_LOCKED.set(state.escrow.total_locked() as i64);
        crate::telemetry::DEX_ESCROW_PENDING.set(state.escrow.count() as i64);
        crate::telemetry::DEX_LIQUIDITY_LOCKED_TOTAL.set(state.escrow.total_locked() as i64);
    }
    let mut ledger = LEDGER.lock().unwrap();
    ledger.adjust(&buy.account, &sell.account, amount as i64);
    ledger.adjust(&sell.account, &buy.account, -(amount as i64));
    let root = state.escrow.status(id).map(|e| e.root);
    let idx = state
        .escrow
        .status(id)
        .map(|e| e.payments.len().saturating_sub(1))
        .unwrap_or(0);
    let mut obj = Map::new();
    obj.insert("proof".to_string(), payment_proof_to_value(&proof));
    if let Some(root) = root {
        obj.insert("root".to_string(), bytes32_to_value(&root));
    }
    obj.insert("idx".to_string(), Value::Number(Number::from(idx as u64)));
    Ok(Value::Object(obj))
}

pub fn escrow_proof(id: u64, idx: usize) -> Option<PaymentProof> {
    let store = STORE.lock().unwrap();
    let state = store.load_escrow_state();
    state.escrow.proof(id, idx)
}
