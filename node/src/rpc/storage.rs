#![forbid(unsafe_code)]

use concurrency::Lazy;
use foundation_serialization::json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;
use storage::{contract::ContractError, StorageContract, StorageOffer};

use crate::storage::pipeline::StoragePipeline;
use crate::storage::repair::RepairRequest;

fn pipeline_path() -> String {
    std::env::var("TB_STORAGE_PIPELINE_DIR").unwrap_or_else(|_| "blobstore".to_string())
}

static CONTRACTS: Lazy<Mutex<BTreeMap<String, StorageContract>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));
static ALLOCATIONS: Lazy<Mutex<BTreeMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Register a new storage contract for the given object and allocate shards.
pub fn upload(
    contract: StorageContract,
    offers: Vec<StorageOffer>,
) -> foundation_serialization::json::Value {
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
) -> foundation_serialization::json::Value {
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

/// Return provider profile snapshots including quotas and recent upload stats.
pub fn provider_profiles() -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let engine = pipeline.engine_summary();
    let legacy_mode = crate::simple_db::legacy_mode();
    let profiles: Vec<foundation_serialization::json::Value> = pipeline
        .provider_profile_snapshots()
        .into_iter()
        .map(|snap| {
            json!({
                "provider": snap.provider,
                "quota_bytes": snap.quota_bytes,
                "preferred_chunk": snap.profile.preferred_chunk,
                "throughput_bps": snap.profile.bw_ewma,
                "rtt_ms": snap.profile.rtt_ewma,
                "loss": snap.profile.loss_ewma,
                "success_rate": snap.profile.success_rate_ewma,
                "recent_failures": snap.profile.recent_failures,
                "total_chunks": snap.profile.total_chunks,
                "total_failures": snap.profile.total_failures,
                "last_upload_bytes": snap.profile.last_upload_bytes,
                "last_upload_secs": snap.profile.last_upload_secs,
                "maintenance": snap.profile.maintenance,
            })
        })
        .collect();
    json!({
        "profiles": profiles,
        "engine": {
            "pipeline": engine.pipeline,
            "rent_escrow": engine.rent_escrow,
            "legacy_mode": legacy_mode,
        }
    })
}

/// Return recent storage repair log entries.
pub fn repair_history(limit: Option<usize>) -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let log = pipeline.repair_log();
    let limit = limit.unwrap_or(25).min(500);
    match log.recent_entries(limit) {
        Ok(entries) => json!({
            "status": "ok",
            "entries": entries,
        }),
        Err(err) => json!({
            "error": err.to_string(),
        }),
    }
}

/// Trigger a manual repair loop iteration and return the summary.
pub fn repair_run() -> foundation_serialization::json::Value {
    let mut pipeline = StoragePipeline::open(&pipeline_path());
    let log = pipeline.repair_log();
    match crate::storage::repair::run_once(pipeline.db_mut(), &log, RepairRequest::default()) {
        Ok(summary) => json!({
            "status": "ok",
            "manifests": summary.manifests,
            "attempts": summary.attempts,
            "successes": summary.successes,
            "failures": summary.failures,
            "skipped": summary.skipped,
            "bytes_repaired": summary.bytes_repaired,
        }),
        Err(err) => json!({
            "error": err.label(),
        }),
    }
}

/// Force a repair attempt for a specific manifest and chunk index.
pub fn repair_chunk(
    manifest_hex: &str,
    chunk_idx: u32,
    force: bool,
) -> foundation_serialization::json::Value {
    let bytes = match hex::decode(manifest_hex) {
        Ok(bytes) => bytes,
        Err(err) => {
            return json!({
                "error": format!("invalid manifest hash: {err}"),
            });
        }
    };
    if bytes.len() != 32 {
        return json!({"error": "manifest hash must be 32 bytes"});
    }
    let mut manifest = [0u8; 32];
    manifest.copy_from_slice(&bytes);

    let mut pipeline = StoragePipeline::open(&pipeline_path());
    let log = pipeline.repair_log();
    let mut request = RepairRequest::default();
    request.manifest = Some(manifest);
    request.chunk = Some(chunk_idx as usize);
    request.force = force;
    match crate::storage::repair::run_once(pipeline.db_mut(), &log, request) {
        Ok(summary) => json!({
            "status": "ok",
            "attempts": summary.attempts,
            "successes": summary.successes,
            "failures": summary.failures,
            "skipped": summary.skipped,
            "bytes_repaired": summary.bytes_repaired,
        }),
        Err(err) => json!({
            "error": err.label(),
        }),
    }
}

/// Toggle maintenance mode for a provider, updating the persisted profile.
pub fn set_provider_maintenance(
    provider: &str,
    maintenance: bool,
) -> foundation_serialization::json::Value {
    let mut pipeline = StoragePipeline::open(&pipeline_path());
    match pipeline.set_provider_maintenance(provider, maintenance) {
        Ok(()) => json!({
            "status": "ok",
            "provider": provider,
            "maintenance": maintenance,
        }),
        Err(err) => json!({"error": err}),
    }
}

/// Return manifest metadata including coding algorithm choices for stored objects.
pub fn manifest_summaries(limit: Option<usize>) -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let max_entries = limit.unwrap_or(100).min(1000);
    let manifests = pipeline.manifest_summaries(max_entries);
    let algorithms = crate::storage::settings::algorithms();
    let policy = json!({
        "erasure": {
            "algorithm": algorithms.erasure(),
            "fallback": algorithms.erasure_fallback(),
            "emergency": algorithms.erasure_emergency(),
        },
        "compression": {
            "algorithm": algorithms.compression(),
            "fallback": algorithms.compression_fallback(),
            "emergency": algorithms.compression_emergency(),
        },
    });
    let entries: Vec<_> = manifests
        .into_iter()
        .map(|entry| {
            json!({
                "manifest": entry.manifest,
                "total_len": entry.total_len,
                "chunk_count": entry.chunk_count,
                "erasure": entry.erasure,
                "compression": entry.compression,
                "encryption": entry.encryption,
                "compression_level": entry.compression_level,
                "erasure_fallback": entry.erasure_fallback,
                "compression_fallback": entry.compression_fallback,
            })
        })
        .collect();
    json!({
        "status": "ok",
        "policy": policy,
        "manifests": entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::json;
    use storage::{StorageContract, StorageOffer};
    use sys::tempfile::tempdir;

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

    #[test]
    fn repair_history_returns_empty_when_log_absent() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let resp = repair_history(Some(5));
        assert_eq!(resp["status"], json!("ok"));
        assert!(resp["entries"].as_array().unwrap().is_empty());
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }

    #[test]
    fn repair_run_handles_empty_database() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let resp = repair_run();
        assert_eq!(resp["status"], json!("ok"));
        assert_eq!(resp["attempts"], json!(0));
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }

    #[test]
    fn repair_chunk_with_unknown_manifest_returns_ok() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let manifest_hex = hex::encode([0u8; 32]);
        let resp = repair_chunk(&manifest_hex, 0, true);
        assert_eq!(resp["status"], json!("ok"));
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }
}
