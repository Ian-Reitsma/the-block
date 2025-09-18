#![forbid(unsafe_code)]

use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;
use storage::{contract::ContractError, StorageContract, StorageOffer};

static CONTRACTS: Lazy<Mutex<BTreeMap<String, StorageContract>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));
static ALLOCATIONS: Lazy<Mutex<BTreeMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Register a new storage contract for the given object and allocate shards.
pub fn upload(contract: StorageContract, offers: Vec<StorageOffer>) -> serde_json::Value {
    let allocation = crate::gateway::storage_alloc::allocate(&offers, contract.shares);
    let providers: Vec<String> = allocation.iter().map(|(p, _)| p.clone()).collect();
    let object_id = contract.object_id.clone();
    let mut store = CONTRACTS.lock().unwrap();
    store.insert(object_id.clone(), contract);
    ALLOCATIONS
        .lock()
        .unwrap()
        .insert(object_id, providers.clone());
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
            Err(ContractError::Expired) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_FAILURE_TOTAL.inc();
                }
                json!({"error": "expired"})
            }
            Err(ContractError::ChallengeFailed) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use storage::{StorageContract, StorageOffer};

    fn reset_state() {
        CONTRACTS.lock().unwrap().clear();
        ALLOCATIONS.lock().unwrap().clear();
    }

    fn sample_contract() -> StorageContract {
        StorageContract {
            object_id: "obj-1".into(),
            provider_id: "prov-a".into(),
            original_bytes: 1_024,
            shares: 4,
            price_per_block: 1,
            start_block: 10,
            retention_blocks: 20,
            next_payment_block: 10,
            accrued: 0,
        }
    }

    fn sample_offers() -> Vec<StorageOffer> {
        vec![
            StorageOffer::new("prov-a".into(), 10, 1, 20),
            StorageOffer::new("prov-b".into(), 5, 1, 20),
        ]
    }

    #[test]
    fn upload_records_contract_and_allocations() {
        reset_state();
        let contract = sample_contract();
        let object_id = contract.object_id.clone();
        let response = upload(contract.clone(), sample_offers());
        assert_eq!(response["status"], json!("ok"));
        let providers_json = response["providers"].as_array().expect("providers array");
        assert_eq!(providers_json.len(), 2);
        assert!(providers_json.iter().any(|p| p == "prov-a"));
        assert!(providers_json.iter().any(|p| p == "prov-b"));

        let stored = CONTRACTS.lock().unwrap();
        let stored_contract = stored.get(&object_id).expect("contract stored");
        assert_eq!(stored_contract.provider_id, contract.provider_id);
        drop(stored);

        let allocations = ALLOCATIONS.lock().unwrap();
        let providers = allocations.get(&object_id).expect("allocation stored");
        assert_eq!(providers.len(), 2);
        assert!(providers.contains(&"prov-a".to_string()));
        assert!(providers.contains(&"prov-b".to_string()));
        drop(allocations);

        reset_state();
    }

    #[test]
    fn challenge_surfaces_success_and_failures() {
        reset_state();
        let contract = sample_contract();
        let object_id = contract.object_id.clone();
        let proof = contract.expected_proof(0);
        upload(contract.clone(), sample_offers());

        let ok = challenge(&object_id, 0, proof, contract.start_block);
        assert_eq!(ok, json!({"status": "ok"}));

        let expired = challenge(
            &object_id,
            0,
            proof,
            contract.start_block + contract.retention_blocks + 1,
        );
        assert_eq!(expired, json!({"error": "expired"}));

        let wrong = challenge(&object_id, 0, [0u8; 32], contract.start_block);
        assert_eq!(wrong, json!({"error": "challenge_failed"}));

        reset_state();
    }
}
