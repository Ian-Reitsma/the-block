#![forbid(unsafe_code)]

use crate::dex::{check_liquidity_rules, storage::EscrowState, DexStore, TrustLedger};
use concurrency::Lazy;
use dex::escrow::PaymentProof;
use std::sync::Mutex;

pub static STORE: Lazy<Mutex<DexStore>> = Lazy::new(|| Mutex::new(DexStore::open("dex_db")));
pub static LEDGER: Lazy<Mutex<TrustLedger>> = Lazy::new(|| Mutex::new(TrustLedger::default()));

pub fn escrow_status(id: u64) -> foundation_serialization::json::Value {
    let store = STORE.lock().unwrap();
    let state: EscrowState = store.load_escrow_state();
    if let Some(entry) = state.escrow.status(id) {
        let mut proofs = Vec::new();
        for (idx, amount) in entry.payments.iter().enumerate() {
            if let Some(p) = state.escrow.proof(id, idx) {
                let mut proof_obj = foundation_serialization::json::Map::new();
                proof_obj.insert(
                    "amount".to_string(),
                    foundation_serialization::json::Value::from(*amount),
                );
                proof_obj.insert(
                    "proof".to_string(),
                    foundation_serialization::json::to_value(p)
                        .unwrap_or(foundation_serialization::json::Value::Null),
                );
                proofs.push(foundation_serialization::json::Value::Object(proof_obj));
            }
        }
        let mut obj = foundation_serialization::json::Map::new();
        obj.insert(
            "from".to_string(),
            foundation_serialization::json::Value::String(entry.from.clone()),
        );
        obj.insert(
            "to".to_string(),
            foundation_serialization::json::Value::String(entry.to.clone()),
        );
        obj.insert(
            "total".to_string(),
            foundation_serialization::json::Value::from(entry.total),
        );
        obj.insert(
            "released".to_string(),
            foundation_serialization::json::Value::from(entry.released),
        );
        obj.insert(
            "outstanding".to_string(),
            foundation_serialization::json::Value::from(entry.total - entry.released),
        );
        obj.insert(
            "proofs".to_string(),
            foundation_serialization::json::Value::Array(proofs),
        );
        foundation_serialization::json::Value::Object(obj)
    } else {
        let mut obj = foundation_serialization::json::Map::new();
        obj.insert(
            "error".to_string(),
            foundation_serialization::json::Value::String("not_found".to_string()),
        );
        foundation_serialization::json::Value::Object(obj)
    }
}

pub fn escrow_release(
    id: u64,
    amount: u64,
) -> Result<foundation_serialization::json::Value, &'static str> {
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
    let mut obj = foundation_serialization::json::Map::new();
    obj.insert(
        "proof".to_string(),
        foundation_serialization::json::to_value(proof.clone())
            .unwrap_or(foundation_serialization::json::Value::Null),
    );
    if let Some(root) = root {
        obj.insert(
            "root".to_string(),
            foundation_serialization::json::to_value(root)
                .unwrap_or(foundation_serialization::json::Value::Null),
        );
    }
    obj.insert(
        "idx".to_string(),
        foundation_serialization::json::Value::from(idx as u64),
    );
    Ok(foundation_serialization::json::Value::Object(obj))
}

pub fn escrow_proof(id: u64, idx: usize) -> Option<PaymentProof> {
    let store = STORE.lock().unwrap();
    let state = store.load_escrow_state();
    state.escrow.proof(id, idx)
}
