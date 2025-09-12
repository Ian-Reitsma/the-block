#![forbid(unsafe_code)]

use crate::dex::{check_liquidity_rules, storage::EscrowState, DexStore, TrustLedger};
use dex::escrow::PaymentProof;
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;

pub static STORE: Lazy<Mutex<DexStore>> = Lazy::new(|| Mutex::new(DexStore::open("dex_db")));
pub static LEDGER: Lazy<Mutex<TrustLedger>> = Lazy::new(|| Mutex::new(TrustLedger::default()));

pub fn escrow_status(id: u64) -> serde_json::Value {
    let store = STORE.lock().unwrap();
    let state: EscrowState = store.load_escrow_state();
    if let Some(entry) = state.escrow.status(id) {
        let mut proofs = Vec::new();
        for (idx, amount) in entry.payments.iter().enumerate() {
            if let Some(p) = state.escrow.proof(id, idx) {
                proofs.push(json!({"amount": amount, "proof": p}));
            }
        }
        json!({
            "from": entry.from,
            "to": entry.to,
            "total": entry.total,
            "released": entry.released,
            "outstanding": entry.total - entry.released,
            "proofs": proofs
        })
    } else {
        json!({"error": "not_found"})
    }
}

pub fn escrow_release(id: u64, amount: u64) -> Result<serde_json::Value, &'static str> {
    let mut store = STORE.lock().unwrap();
    let mut state = store.load_escrow_state();
    let (buy, sell, _qty, locked_at) = state
        .locks
        .get(&id)
        .cloned()
        .ok_or("not_found")?;
    check_liquidity_rules(locked_at)?;
    let proof = state
        .escrow
        .release(id, amount)
        .ok_or("invalid_release")?;
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
    Ok(json!({"proof": proof.clone(), "root": root, "idx": idx}))
}

pub fn escrow_proof(id: u64, idx: usize) -> Option<PaymentProof> {
    let store = STORE.lock().unwrap();
    let state = store.load_escrow_state();
    state.escrow.proof(id, idx)
}
