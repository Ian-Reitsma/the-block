#![forbid(unsafe_code)]

use crate::dex::DexStore;
use dex::escrow::{Escrow, PaymentProof};
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;

pub static STORE: Lazy<Mutex<DexStore>> = Lazy::new(|| Mutex::new(DexStore::open("dex_db")));

pub fn escrow_status(id: u64) -> serde_json::Value {
    let store = STORE.lock().unwrap();
    let esc: Escrow = store.load_escrow();
    if let Some(e) = esc.status(id) {
        json!({"from": e.from, "to": e.to, "total": e.total, "released": e.released})
    } else {
        json!({"error": "not_found"})
    }
}

pub fn escrow_release(id: u64, amount: u64) -> Result<serde_json::Value, &'static str> {
    let mut store = STORE.lock().unwrap();
    let mut esc = store.load_escrow();
    let proof = esc.release(id, amount).ok_or("invalid_release")?;
    let root = esc.status(id).map(|e| e.root);
    store.save_escrow(&esc);
    Ok(json!({"proof": proof, "root": root}))
}

pub fn escrow_proof(id: u64, idx: usize) -> Option<PaymentProof> {
    let store = STORE.lock().unwrap();
    let esc = store.load_escrow();
    esc.proof(id, idx)
}
