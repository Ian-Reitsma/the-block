#![forbid(unsafe_code)]

use crate::vm::contracts::htlc::Htlc;
use concurrency::Lazy;
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

pub fn status(id: u64) -> foundation_serialization::json::Value {
    let store = STORE.lock().unwrap();
    if let Some(h) = store.get(&id) {
        foundation_serialization::json!({
            "hash": hex::encode(&h.hash),
            "timeout": h.timeout,
            "redeemed": h.redeemed,
        })
    } else {
        foundation_serialization::json!({"error": "not_found"})
    }
}

pub fn refund(id: u64, now: u64) -> foundation_serialization::json::Value {
    let mut store = STORE.lock().unwrap();
    if let Some(h) = store.get_mut(&id) {
        if h.refund(now) {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::HTLC_REFUNDED_TOTAL.inc();
            }
            foundation_serialization::json!({"status": "refunded"})
        } else {
            foundation_serialization::json!({"error": "not_refundable"})
        }
    } else {
        foundation_serialization::json!({"error": "not_found"})
    }
}
