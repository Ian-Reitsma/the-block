#![forbid(unsafe_code)]

use concurrency::Lazy;
use foundation_serialization::json::{Map, Number, Value};
use std::sync::Arc;
use storage::{contract::ContractError, merkle_proof::MerkleProof, StorageContract, StorageOffer};
use storage_market::{
    ProofOutcome, ProofRecord, ReplicaIncentive, StorageMarket, StorageMarketError,
};

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

fn market_path() -> String {
    std::env::var("TB_STORAGE_MARKET_DIR").unwrap_or_else(|_| "storage_market".to_string())
}

use concurrency::{mutex, MutexExt, MutexT};

type StorageMarketHandle = Arc<MutexT<StorageMarket>>;

static MARKET: Lazy<StorageMarketHandle> = Lazy::new(|| {
    let path = market_path();
    let market = StorageMarket::open(&path)
        .unwrap_or_else(|err| panic!("failed to open storage market at {path}: {err}"));
    Arc::new(mutex(market))
});

fn market_error_value(err: StorageMarketError) -> Value {
    error_value(err.to_string())
}

fn outcome_to_string(outcome: &ProofOutcome) -> &'static str {
    match outcome {
        ProofOutcome::Success => "success",
        ProofOutcome::Failure => "failure",
    }
}

fn proof_record_value(record: &ProofRecord) -> Value {
    let mut map = Map::new();
    map.insert("object_id".into(), Value::String(record.object_id.clone()));
    map.insert("provider".into(), Value::String(record.provider_id.clone()));
    map.insert(
        "remaining_deposit".into(),
        Value::Number(Number::from(record.remaining_deposit)),
    );
    map.insert(
        "accrued_ct".into(),
        Value::Number(Number::from(record.amount_accrued_ct)),
    );
    map.insert(
        "proof_successes".into(),
        Value::Number(Number::from(record.proof_successes)),
    );
    map.insert(
        "proof_failures".into(),
        Value::Number(Number::from(record.proof_failures)),
    );
    map.insert(
        "outcome".into(),
        Value::String(outcome_to_string(&record.outcome).to_string()),
    );
    map.insert(
        "slashed_ct".into(),
        Value::Number(Number::from(record.slashed_ct)),
    );
    Value::Object(map)
}

fn replica_value(object_id: &str, contract: &StorageContract, replica: &ReplicaIncentive) -> Value {
    let mut map = Map::new();
    map.insert("object_id".into(), Value::String(object_id.to_string()));
    map.insert(
        "provider".into(),
        Value::String(replica.provider_id.clone()),
    );
    map.insert(
        "shares".into(),
        Value::Number(Number::from(replica.allocated_shares as u64)),
    );
    map.insert(
        "price_per_block".into(),
        Value::Number(Number::from(replica.price_per_block)),
    );
    map.insert(
        "deposit".into(),
        Value::Number(Number::from(replica.deposit)),
    );
    map.insert(
        "proof_successes".into(),
        Value::Number(Number::from(replica.proof_successes)),
    );
    map.insert(
        "proof_failures".into(),
        Value::Number(Number::from(replica.proof_failures)),
    );
    map.insert(
        "contract_accrued_ct".into(),
        Value::Number(Number::from(contract.accrued)),
    );
    map.insert(
        "contract_total_deposit".into(),
        Value::Number(Number::from(contract.total_deposit)),
    );
    map.insert(
        "last_payment_block".into(),
        contract
            .last_payment_block
            .map(|block| Value::Number(Number::from(block)))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "last_proof_block".into(),
        replica
            .last_proof_block
            .map(|block| Value::Number(Number::from(block)))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "last_outcome".into(),
        replica
            .last_outcome
            .as_ref()
            .map(|outcome| Value::String(outcome_to_string(outcome).to_string()))
            .unwrap_or(Value::Null),
    );
    Value::Object(map)
}

fn compute_price_distribution(
    total_price: u64,
    allocation: &[(String, u16)],
    total_shares: u16,
) -> Vec<u64> {
    if total_price == 0 || allocation.is_empty() || total_shares == 0 {
        return vec![0; allocation.len()];
    }
    let total_price = u128::from(total_price);
    let total_shares = u128::from(total_shares);
    let mut weights: Vec<(usize, u128, u128)> = allocation
        .iter()
        .enumerate()
        .map(|(idx, (_, shares))| {
            let share = u128::from(*shares);
            let numerator = total_price * share;
            let base = numerator / total_shares;
            let remainder = numerator % total_shares;
            (idx, base, remainder)
        })
        .collect();
    let mut distribution: Vec<u64> = weights
        .iter()
        .map(|(_, base, _)| (*base).min(u128::from(u64::MAX)) as u64)
        .collect();
    let allocated: u128 = distribution.iter().map(|value| u128::from(*value)).sum();
    let mut leftover = total_price.saturating_sub(allocated);
    if leftover > 0 && !weights.is_empty() {
        weights.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        let len = weights.len();
        let mut idx = 0usize;
        while leftover > 0 {
            let target = weights[idx % len].0;
            distribution[target] = distribution[target].saturating_add(1);
            leftover = leftover.saturating_sub(1);
            idx += 1;
        }
    }
    distribution
}

fn build_replicas(
    contract: &StorageContract,
    allocation: &[(String, u16)],
) -> Vec<ReplicaIncentive> {
    let total_shares = if contract.shares == 0 {
        1
    } else {
        contract.shares
    };
    let price_distribution =
        compute_price_distribution(contract.price_per_block, allocation, total_shares);
    allocation
        .iter()
        .zip(price_distribution.into_iter())
        .map(|((provider, shares), price)| {
            let deposit = u128::from(price)
                .saturating_mul(u128::from(contract.retention_blocks))
                .min(u128::from(u64::MAX)) as u64;
            ReplicaIncentive::new(provider.clone(), *shares, price, deposit)
        })
        .collect()
}

/// Register a new storage contract for the given object and allocate shards.
pub fn upload(
    contract: StorageContract,
    offers: Vec<StorageOffer>,
) -> foundation_serialization::json::Value {
    let allocation = crate::gateway::storage_alloc::allocate(&offers, contract.shares);
    if allocation.is_empty() {
        return error_value("no_providers");
    }
    let replicas = build_replicas(&contract, &allocation);
    let providers: Vec<Value> = allocation
        .iter()
        .map(|(provider, _)| Value::String(provider.clone()))
        .collect();
    match MARKET.guard().register_contract(contract, replicas.clone()) {
        Ok(record) => {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::STORAGE_CONTRACT_CREATED_TOTAL.inc();
            }
            let replica_values: Vec<Value> = replicas
                .iter()
                .map(|replica| replica_value(&record.contract.object_id, &record.contract, replica))
                .collect();
            json_object(vec![
                ("status", Value::String("ok".to_string())),
                ("providers", Value::Array(providers)),
                ("replicas", Value::Array(replica_values)),
                (
                    "total_deposit",
                    Value::Number(Number::from(record.contract.total_deposit)),
                ),
            ])
        }
        Err(err) => market_error_value(err),
    }
}

/// Challenge a provider to prove retrievability of an object.
pub fn challenge(
    object_id: &str,
    provider_id: Option<&str>,
    chunk_idx: u64,
    chunk_data: &[u8],
    proof: &MerkleProof,
    current_block: u64,
) -> foundation_serialization::json::Value {
    let record = match MARKET.guard().load_contract(object_id) {
        Ok(Some(record)) => record,
        Ok(None) => return error_value("not_found"),
        Err(err) => return market_error_value(err),
    };
    match record
        .contract
        .verify_proof(chunk_idx, chunk_data, proof, current_block)
    {
        Ok(()) => {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::RETRIEVAL_SUCCESS_TOTAL.inc();
            }
            match MARKET
                .guard()
                .record_proof_outcome(object_id, provider_id, current_block, true)
            {
                Ok(proof_record) => json_object(vec![
                    ("status", Value::String("ok".to_string())),
                    ("proof", proof_record_value(&proof_record)),
                ]),
                Err(err) => market_error_value(err),
            }
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
            let targeted_provider = provider_id.map(|p| p.to_string()).or_else(|| {
                record
                    .replicas
                    .first()
                    .map(|replica| replica.provider_id.clone())
            });
            if let Some(provider) = targeted_provider {
                let reputation = crate::compute_market::scheduler::reputation_get(&provider);
                let updated = reputation.saturating_sub(1);
                crate::compute_market::scheduler::merge_reputation(&provider, updated, u64::MAX);
            }
            let _ =
                MARKET
                    .guard()
                    .record_proof_outcome(object_id, provider_id, current_block, false);
            error_value("challenge_failed")
        }
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

/// Return the incentive snapshot for all registered replicas.
pub fn incentives_snapshot() -> foundation_serialization::json::Value {
    match MARKET.guard().contracts() {
        Ok(records) => {
            let mut replicas = Vec::new();
            for record in records {
                for replica in &record.replicas {
                    replicas.push(replica_value(
                        &record.contract.object_id,
                        &record.contract,
                        replica,
                    ));
                }
            }
            json_object(vec![
                ("status", Value::String("ok".to_string())),
                ("replicas", Value::Array(replicas)),
            ])
        }
        Err(err) => market_error_value(err),
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

/// Drain pending storage market receipts for block inclusion.
///
/// This function accesses the global storage market instance, drains all pending
/// settlement receipts, and converts them to the canonical `StorageReceipt` format
/// used in blocks.
pub fn drain_storage_receipts() -> Vec<crate::receipts::StorageReceipt> {
    let receipts: Vec<_> = MARKET
        .guard()
        .drain_receipts()
        .into_iter()
        .map(|r| crate::receipts::StorageReceipt {
            contract_id: r.contract_id,
            provider: r.provider,
            bytes: r.bytes,
            price: r.price,
            block_height: r.block_height,
            provider_escrow: r.provider_escrow,
            provider_signature: vec![],
            signature_nonce: 0,
        })
        .collect();

    // Record telemetry for drain operation
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::receipts::RECEIPT_DRAIN_OPERATIONS_TOTAL.inc();
        if !receipts.is_empty() {
            diagnostics::tracing::debug!(
                receipt_count = receipts.len(),
                market = "storage",
                "Drained storage receipts"
            );
        }
    }

    receipts
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json::{
        Map as JsonMap, Number as JsonNumber, Value as JsonValue,
    };
    use std::sync::Once;
    use storage::merkle_proof::MerkleTree;
    use storage::{StorageContract, StorageOffer};
    use sys::tempfile::tempdir;

    static INIT_MARKET: Once = Once::new();

    fn json_string(value: &str) -> JsonValue {
        JsonValue::String(value.to_owned())
    }

    fn json_number(value: i64) -> JsonValue {
        JsonValue::Number(JsonNumber::from(value))
    }

    fn ensure_market_dir() {
        INIT_MARKET.call_once(|| {
            let dir = tempdir().expect("tempdir");
            std::env::set_var("TB_STORAGE_MARKET_DIR", dir.path());
            Box::leak(Box::new(dir));
        });
    }

    fn reset_state() {
        ensure_market_dir();
        // Note: MARKET is a Lazy static that cannot be cleared once initialized.
        // Tests rely on isolated temp directories set via TB_STORAGE_MARKET_DIR.
    }

    fn json_object(entries: &[(&str, JsonValue)]) -> JsonValue {
        let mut map = JsonMap::new();
        for (key, value) in entries {
            map.insert((*key).to_string(), value.clone());
        }
        JsonValue::Object(map)
    }

    fn demo_chunks() -> Vec<Vec<u8>> {
        vec![
            b"chunk0".to_vec(),
            b"chunk1".to_vec(),
            b"chunk2".to_vec(),
            b"chunk3".to_vec(),
        ]
    }

    fn sample_contract_with_tree() -> (StorageContract, Vec<Vec<u8>>, MerkleTree) {
        let chunks = demo_chunks();
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs).expect("build tree");
        let contract = StorageContract {
            object_id: "obj-1".into(),
            provider_id: "prov-a".into(),
            original_bytes: 1_024,
            shares: 4,
            price_per_block: 4,
            start_block: 10,
            retention_blocks: 20,
            next_payment_block: 10,
            accrued: 0,
            total_deposit: 0,
            last_payment_block: None,
            storage_root: tree.root,
        };
        (contract, chunks, tree)
    }

    fn sample_offers() -> Vec<StorageOffer> {
        vec![
            StorageOffer::new("prov-a".into(), 10, 1, 20),
            StorageOffer::new("prov-b".into(), 5, 1, 20),
        ]
    }

    #[test]
    fn upload_records_contract_and_replicas() {
        reset_state();
        let (contract, _chunks, _tree) = sample_contract_with_tree();
        let response = upload(contract.clone(), sample_offers());
        assert_eq!(response["status"], json_string("ok"));
        let providers_json = response["providers"].as_array().expect("providers array");
        assert_eq!(providers_json.len(), 2);
        let replicas = response["replicas"].as_array().expect("replicas array");
        assert_eq!(replicas.len(), 2);
        let deposit = response["total_deposit"].as_u64().expect("deposit value");
        assert!(deposit > 0);
        reset_state();
    }

    #[test]
    fn challenge_surfaces_success_and_failures() {
        reset_state();
        let (contract, chunks, tree) = sample_contract_with_tree();
        let object_id = contract.object_id.clone();
        upload(contract.clone(), sample_offers());
        let chunk_idx = 0;
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let proof = tree
            .generate_proof(chunk_idx, &chunk_refs)
            .expect("generate proof");
        let chunk_data = &chunks[chunk_idx as usize];

        let ok = challenge(
            &object_id,
            None,
            chunk_idx,
            chunk_data,
            &proof,
            contract.start_block,
        );
        assert_eq!(ok["status"], json_string("ok"));
        assert!(ok["proof"].is_object());

        let expired = challenge(
            &object_id,
            None,
            chunk_idx,
            chunk_data,
            &proof,
            contract.start_block + contract.retention_blocks + 1,
        );
        assert_eq!(expired, json_object(&[("error", json_string("expired"))]));

        let wrong = challenge(
            &object_id,
            None,
            chunk_idx,
            b"wrong".as_ref(),
            &proof,
            contract.start_block,
        );
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
