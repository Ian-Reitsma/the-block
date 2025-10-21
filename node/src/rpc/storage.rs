#![forbid(unsafe_code)]

use concurrency::Lazy;
use foundation_serialization::json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;
use storage::{contract::ContractError, StorageContract, StorageOffer};

use crate::storage::pipeline::StoragePipeline;
use crate::storage::repair::repair_log_entry_to_value;
use crate::storage::repair::RepairRequest;

fn json_object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn status_value(status: &'static str) -> Value {
    json_object(vec![("status", Value::String(status.to_string()))])
}

fn error_value(message: impl Into<String>) -> Value {
    json_object(vec![("error", Value::String(message.into()))])
}

fn number_from_usize(value: usize) -> Value {
    Value::Number(Number::from(value as u64))
}

fn number_from_option_i32(value: Option<i32>) -> Value {
    value
        .map(|inner| Value::Number(Number::from(inner)))
        .unwrap_or(Value::Null)
}

fn string_from_option(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}

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
    json_object(vec![
        ("status", Value::String("ok".to_string())),
        (
            "providers",
            Value::Array(providers.into_iter().map(Value::String).collect()),
        ),
    ])
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
                status_value("ok")
            }
            Err(ContractError::Expired) => {
                #[cfg(feature = "telemetry")]
                {
                    crate::telemetry::RETRIEVAL_FAILURE_TOTAL.inc();
                }
                error_value("expired")
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
                error_value("challenge_failed")
            }
        }
    } else {
        error_value("not_found")
    }
}

/// Return provider profile snapshots including quotas and recent upload stats.
pub fn provider_profiles() -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let engine = pipeline.engine_summary();
    let legacy_mode = crate::simple_db::legacy_mode();
    let profiles: Vec<Value> = pipeline
        .provider_profile_snapshots()
        .into_iter()
        .map(|snap| {
            json_object(vec![
                ("provider", Value::String(snap.provider)),
                ("quota_bytes", Value::Number(Number::from(snap.quota_bytes))),
                (
                    "preferred_chunk",
                    Value::Number(Number::from(snap.profile.preferred_chunk)),
                ),
                ("throughput_bps", Value::from(snap.profile.bw_ewma)),
                ("rtt_ms", Value::from(snap.profile.rtt_ewma)),
                ("loss", Value::from(snap.profile.loss_ewma)),
                ("success_rate", Value::from(snap.profile.success_rate_ewma)),
                (
                    "recent_failures",
                    Value::Number(Number::from(snap.profile.recent_failures)),
                ),
                (
                    "total_chunks",
                    Value::Number(Number::from(snap.profile.total_chunks)),
                ),
                (
                    "total_failures",
                    Value::Number(Number::from(snap.profile.total_failures)),
                ),
                (
                    "last_upload_bytes",
                    Value::Number(Number::from(snap.profile.last_upload_bytes)),
                ),
                (
                    "last_upload_secs",
                    Value::Number(Number::from(snap.profile.last_upload_secs)),
                ),
                ("maintenance", Value::Bool(snap.profile.maintenance)),
            ])
        })
        .collect();
    let mut engine_map = Map::new();
    engine_map.insert(
        "pipeline".to_string(),
        Value::String(engine.pipeline.clone()),
    );
    engine_map.insert(
        "rent_escrow".to_string(),
        Value::String(engine.rent_escrow.clone()),
    );
    engine_map.insert("legacy_mode".to_string(), Value::Bool(legacy_mode));
    json_object(vec![
        ("profiles", Value::Array(profiles)),
        ("engine", Value::Object(engine_map)),
    ])
}

/// Return recent storage repair log entries.
pub fn repair_history(limit: Option<usize>) -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let log = pipeline.repair_log();
    let limit = limit.unwrap_or(25).min(500);
    match log.recent_entries(limit) {
        Ok(entries) => {
            let values: Vec<Value> = entries
                .into_iter()
                .map(|entry| repair_log_entry_to_value(&entry))
                .collect();
            json_object(vec![
                ("status", Value::String("ok".to_string())),
                ("entries", Value::Array(values)),
            ])
        }
        Err(err) => error_value(err.to_string()),
    }
}

/// Trigger a manual repair loop iteration and return the summary.
pub fn repair_run() -> foundation_serialization::json::Value {
    let mut pipeline = StoragePipeline::open(&pipeline_path());
    let log = pipeline.repair_log();
    match crate::storage::repair::run_once(pipeline.db_mut(), &log, RepairRequest::default()) {
        Ok(summary) => json_object(vec![
            ("status", Value::String("ok".to_string())),
            ("manifests", number_from_usize(summary.manifests)),
            ("attempts", number_from_usize(summary.attempts)),
            ("successes", number_from_usize(summary.successes)),
            ("failures", number_from_usize(summary.failures)),
            ("skipped", number_from_usize(summary.skipped)),
            (
                "bytes_repaired",
                Value::Number(Number::from(summary.bytes_repaired)),
            ),
        ]),
        Err(err) => error_value(err.label()),
    }
}

/// Force a repair attempt for a specific manifest and chunk index.
pub fn repair_chunk(
    manifest_hex: &str,
    chunk_idx: u32,
    force: bool,
) -> foundation_serialization::json::Value {
    let bytes = match crypto_suite::hex::decode(manifest_hex) {
        Ok(bytes) => bytes,
        Err(err) => {
            return error_value(format!("invalid manifest hash: {err}"));
        }
    };
    if bytes.len() != 32 {
        return error_value("manifest hash must be 32 bytes");
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
        Ok(summary) => json_object(vec![
            ("status", Value::String("ok".to_string())),
            ("attempts", number_from_usize(summary.attempts)),
            ("successes", number_from_usize(summary.successes)),
            ("failures", number_from_usize(summary.failures)),
            ("skipped", number_from_usize(summary.skipped)),
            (
                "bytes_repaired",
                Value::Number(Number::from(summary.bytes_repaired)),
            ),
        ]),
        Err(err) => error_value(err.label()),
    }
}

/// Toggle maintenance mode for a provider, updating the persisted profile.
pub fn set_provider_maintenance(
    provider: &str,
    maintenance: bool,
) -> foundation_serialization::json::Value {
    let mut pipeline = StoragePipeline::open(&pipeline_path());
    match pipeline.set_provider_maintenance(provider, maintenance) {
        Ok(()) => json_object(vec![
            ("status", Value::String("ok".to_string())),
            ("provider", Value::String(provider.to_string())),
            ("maintenance", Value::Bool(maintenance)),
        ]),
        Err(err) => error_value(err.to_string()),
    }
}

/// Return manifest metadata including coding algorithm choices for stored objects.
pub fn manifest_summaries(limit: Option<usize>) -> foundation_serialization::json::Value {
    let pipeline = StoragePipeline::open(&pipeline_path());
    let max_entries = limit.unwrap_or(100).min(1000);
    let manifests = pipeline.manifest_summaries(max_entries);
    let algorithms = crate::storage::settings::algorithms();
    let policy = json_object(vec![
        (
            "erasure",
            Value::Object({
                let mut map = Map::new();
                map.insert(
                    "algorithm".to_string(),
                    Value::String(algorithms.erasure().to_string()),
                );
                map.insert(
                    "fallback".to_string(),
                    Value::Bool(algorithms.erasure_fallback()),
                );
                map.insert(
                    "emergency".to_string(),
                    Value::Bool(algorithms.erasure_emergency()),
                );
                map
            }),
        ),
        (
            "compression",
            Value::Object({
                let mut map = Map::new();
                map.insert(
                    "algorithm".to_string(),
                    Value::String(algorithms.compression().to_string()),
                );
                map.insert(
                    "fallback".to_string(),
                    Value::Bool(algorithms.compression_fallback()),
                );
                map.insert(
                    "emergency".to_string(),
                    Value::Bool(algorithms.compression_emergency()),
                );
                map
            }),
        ),
    ]);
    let entries: Vec<_> = manifests
        .into_iter()
        .map(|entry| {
            json_object(vec![
                ("manifest", Value::String(entry.manifest)),
                ("total_len", Value::Number(Number::from(entry.total_len))),
                (
                    "chunk_count",
                    Value::Number(Number::from(entry.chunk_count)),
                ),
                ("erasure", Value::String(entry.erasure)),
                ("compression", Value::String(entry.compression)),
                ("encryption", string_from_option(entry.encryption)),
                (
                    "compression_level",
                    number_from_option_i32(entry.compression_level),
                ),
                ("erasure_fallback", Value::Bool(entry.erasure_fallback)),
                (
                    "compression_fallback",
                    Value::Bool(entry.compression_fallback),
                ),
            ])
        })
        .collect();
    json_object(vec![
        ("status", Value::String("ok".to_string())),
        ("policy", policy),
        ("manifests", Value::Array(entries)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::{
        Map as JsonMap, Number as JsonNumber, Value as JsonValue,
    };
    use storage::{StorageContract, StorageOffer};
    use sys::tempfile::tempdir;

    fn json_string(value: &str) -> JsonValue {
        JsonValue::String(value.to_owned())
    }

    fn json_number(value: i64) -> JsonValue {
        JsonValue::Number(JsonNumber::from(value))
    }

    fn json_object(entries: &[(&str, JsonValue)]) -> JsonValue {
        let mut map = JsonMap::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), value.clone());
        }
        JsonValue::Object(map)
    }

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
        assert_eq!(response["status"], json_string("ok"));
        let providers_json = response["providers"].as_array().expect("providers array");
        assert_eq!(providers_json.len(), 2);
        assert!(providers_json.iter().any(|p| p.as_str() == Some("prov-a")));
        assert!(providers_json.iter().any(|p| p.as_str() == Some("prov-b")));

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
        assert_eq!(ok, json_object(&[("status", json_string("ok"))]));

        let expired = challenge(
            &object_id,
            0,
            proof,
            contract.start_block + contract.retention_blocks + 1,
        );
        assert_eq!(expired, json_object(&[("error", json_string("expired"))]));

        let wrong = challenge(&object_id, 0, [0u8; 32], contract.start_block);
        assert_eq!(
            wrong,
            json_object(&[("error", json_string("challenge_failed"))])
        );

        reset_state();
    }

    #[test]
    fn repair_history_returns_empty_when_log_absent() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let resp = repair_history(Some(5));
        assert_eq!(resp["status"], json_string("ok"));
        assert!(resp["entries"].as_array().unwrap().is_empty());
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }

    #[test]
    fn repair_run_handles_empty_database() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let resp = repair_run();
        assert_eq!(resp["status"], json_string("ok"));
        assert_eq!(resp["attempts"], json_number(0));
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }

    #[test]
    fn repair_chunk_with_unknown_manifest_returns_ok() {
        let dir = tempdir().expect("dir");
        std::env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path().to_str().unwrap());
        let manifest_hex = crypto_suite::hex::encode([0u8; 32]);
        let resp = repair_chunk(&manifest_hex, 0, true);
        assert_eq!(resp["status"], json_string("ok"));
        std::env::remove_var("TB_STORAGE_PIPELINE_DIR");
    }
}
