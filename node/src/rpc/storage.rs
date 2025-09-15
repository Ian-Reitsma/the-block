#![forbid(unsafe_code)]

use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;
use storage::{StorageContract, StorageOffer};

static CONTRACTS: Lazy<Mutex<BTreeMap<String, StorageContract>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));
static ALLOCATIONS: Lazy<Mutex<BTreeMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Register a new storage contract for the given object and allocate shards.
pub fn upload(contract: StorageContract, offers: Vec<StorageOffer>) -> serde_json::Value {
    let allocation = crate::gateway::storage_alloc::allocate(&offers, contract.shares);
    let providers: Vec<String> = allocation.iter().map(|(p, _)| p.clone()).collect();
    let mut store = CONTRACTS.lock().unwrap();
    store.insert(contract.object_id.clone(), contract);
    ALLOCATIONS
        .lock()
        .unwrap()
        .insert(contract.object_id.clone(), providers.clone());
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::STORAGE_CONTRACT_CREATED_TOTAL.inc();
    }
    json!({"status": "ok", "providers": providers})
}

/// Challenge a provider to prove retrievability of an object.
pub fn challenge(
    object_id: &str,
    chunk_idx: u64,
    proof: [u8; 32],
    current_block: u64,
) -> serde_json::Value {
    let store = CONTRACTS.lock().unwrap();
    if let Some(contract) = store.get(object_id) {
        match contract.verify_proof(chunk_idx, proof, current_block) {
            Ok(()) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_SUCCESS_TOTAL.inc();
                }
                json!({"status": "ok"})
            }
            Err(storage::ContractError::Expired) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_FAILURE_TOTAL.inc();
                }
                json!({"error": "expired"})
            }
            Err(storage::ContractError::ChallengeFailed) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_FAILURE_TOTAL.inc();
                }
                if let Some(provs) = ALLOCATIONS.lock().unwrap().get(object_id) {
                    if let Some(first) = provs.first() {
                        let rep = crate::compute_market::scheduler::reputation_get(first) - 1;
                        crate::compute_market::scheduler::merge_reputation(first, rep, u64::MAX);
                    }
                }
                json!({"error": "challenge_failed"})
            }
        }
    } else {
        json!({"error": "not_found"})
    }
}
