#![forbid(unsafe_code)]

use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;
use storage::StorageContract;

static CONTRACTS: Lazy<Mutex<BTreeMap<String, StorageContract>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Register a new storage contract for the given object.
pub fn upload(contract: StorageContract) -> serde_json::Value {
    let mut store = CONTRACTS.lock().unwrap();
    store.insert(contract.object_id.clone(), contract);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::STORAGE_CONTRACT_CREATED_TOTAL.inc();
    }
    json!({"status": "ok"})
}

/// Challenge a provider to prove retrievability of an object.
pub fn challenge(object_id: &str, current_block: u64) -> serde_json::Value {
    let mut store = CONTRACTS.lock().unwrap();
    if let Some(contract) = store.get(object_id) {
        match contract.is_active(current_block) {
            Ok(()) => json!({"status": "ok"}),
            Err(_) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_FAILURE_TOTAL.inc();
                }
                json!({"error": "expired"})
            }
        }
    } else {
        json!({"error": "not_found"})
    }
}
