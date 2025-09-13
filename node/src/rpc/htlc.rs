#![forbid(unsafe_code)]

use crate::vm::contracts::htlc::Htlc;
use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;

static STORE: Lazy<Mutex<BTreeMap<u64, Htlc>>> = Lazy::new(|| Mutex::new(BTreeMap::new()));

pub fn insert(id: u64, h: Htlc) {
    let mut store = STORE.lock().unwrap();
    store.insert(id, h);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::HTLC_CREATED_TOTAL.inc();
    }
}

pub fn status(id: u64) -> serde_json::Value {
    let store = STORE.lock().unwrap();
    if let Some(h) = store.get(&id) {
        json!({
            "hash": hex::encode(&h.hash),
            "timeout": h.timeout,
            "redeemed": h.redeemed,
        })
    } else {
        json!({"error": "not_found"})
    }
}

pub fn refund(id: u64, now: u64) -> serde_json::Value {
    let mut store = STORE.lock().unwrap();
    if let Some(h) = store.get_mut(&id) {
        if h.refund(now) {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::HTLC_REFUNDED_TOTAL.inc();
            }
            json!({"status": "refunded"})
        } else {
            json!({"error": "not_refundable"})
        }
    } else {
        json!({"error": "not_found"})
    }
}
