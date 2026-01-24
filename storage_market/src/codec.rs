#![forbid(unsafe_code)]

use foundation_serialization::json::{self, Map, Number, Value};

use crate::{ContractRecord, ProofOutcome, ProviderProfile, ReplicaIncentive, StorageMarketError};
use storage::{merkle_proof::MerkleRoot, StorageContract};

const CONTRACT_KEY: &str = "contract";
const REPLICAS_KEY: &str = "replicas";

pub fn serialize_contract_record(record: &ContractRecord) -> Result<Vec<u8>, StorageMarketError> {
    let mut outer = Map::new();
    outer.insert(
        CONTRACT_KEY.to_string(),
        storage_contract_to_value(&record.contract),
    );
    let replicas = record
        .replicas
        .iter()
        .map(replica_to_value)
        .collect::<Vec<_>>();
    outer.insert(REPLICAS_KEY.to_string(), Value::Array(replicas));
    Ok(json::to_vec_value(&Value::Object(outer)))
}

const PROVIDER_ID_KEY: &str = "provider_id";
const REGION_KEY: &str = "region";
const MAX_CAPACITY_KEY: &str = "max_capacity_bytes";
const PRICE_KEY: &str = "price_per_block";
const DEPOSIT_KEY: &str = "escrow_deposit";
const LATENCY_KEY: &str = "latency_ms";
const TAGS_KEY: &str = "tags";
const SUCC_KEY: &str = "proof_successes";
const FAIL_KEY: &str = "proof_failures";
const LAST_SEEN_KEY: &str = "last_seen_block";

pub fn serialize_provider_profile(
    profile: &ProviderProfile,
) -> Result<Vec<u8>, StorageMarketError> {
    let mut map = Map::new();
    map.insert(
        PROVIDER_ID_KEY.to_string(),
        Value::String(profile.provider_id.clone()),
    );
    map.insert(
        REGION_KEY.to_string(),
        profile
            .region
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        MAX_CAPACITY_KEY.to_string(),
        Value::from(profile.max_capacity_bytes),
    );
    map.insert(PRICE_KEY.to_string(), Value::from(profile.price_per_block));
    map.insert(DEPOSIT_KEY.to_string(), Value::from(profile.escrow_deposit));
    map.insert(
        LATENCY_KEY.to_string(),
        profile.latency_ms.map(Value::from).unwrap_or(Value::Null),
    );
    map.insert(
        TAGS_KEY.to_string(),
        Value::Array(
            profile
                .tags
                .iter()
                .map(|tag| Value::String(tag.clone()))
                .collect(),
        ),
    );
    map.insert(SUCC_KEY.to_string(), Value::from(profile.proof_successes));
    map.insert(FAIL_KEY.to_string(), Value::from(profile.proof_failures));
    map.insert(
        LAST_SEEN_KEY.to_string(),
        profile
            .last_seen_block
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    Ok(json::to_vec_value(&Value::Object(map)))
}

pub fn deserialize_provider_profile(bytes: &[u8]) -> Result<ProviderProfile, StorageMarketError> {
    let value = json::value_from_slice(bytes)
        .map_err(|err| StorageMarketError::Serialization(err.to_string()))?;
    let mut map = expect_object(value, "provider profile")?;
    let provider_id = take_string(&mut map, PROVIDER_ID_KEY, "provider profile")?;
    let region = take_optional_string(&mut map, REGION_KEY, "provider profile")?;
    let max_capacity_bytes = take_u64(&mut map, MAX_CAPACITY_KEY, "provider profile")?;
    let price_per_block = take_u64(&mut map, PRICE_KEY, "provider profile")?;
    let escrow_deposit = take_u64(&mut map, DEPOSIT_KEY, "provider profile")?;
    let latency = take_optional_u64(&mut map, LATENCY_KEY, "provider profile")?
        .map(|value| value.min(u32::MAX as u64) as u32);
    let tags = take_string_array(&mut map, TAGS_KEY, "provider profile")?;
    let proof_successes = take_u64_default(&mut map, SUCC_KEY, "provider profile", 0)?;
    let proof_failures = take_u64_default(&mut map, FAIL_KEY, "provider profile", 0)?;
    let last_seen_block = take_optional_u64(&mut map, LAST_SEEN_KEY, "provider profile")?;
    Ok(ProviderProfile {
        provider_id,
        region,
        max_capacity_bytes,
        price_per_block,
        escrow_deposit,
        latency_ms: latency,
        tags,
        proof_successes,
        proof_failures,
        last_seen_block,
    })
}

pub fn deserialize_contract_record(bytes: &[u8]) -> Result<ContractRecord, StorageMarketError> {
    let value = json::value_from_slice(bytes)
        .map_err(|err| StorageMarketError::Serialization(err.to_string()))?;
    let mut map = expect_object(value, "contract record")?;
    let contract_value = map
        .remove(CONTRACT_KEY)
        .ok_or_else(|| field_err(CONTRACT_KEY, "contract record"))?;
    let contract = storage_contract_from_value(contract_value)?;
    let replicas = match map.remove(REPLICAS_KEY) {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(replica_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        Some(Value::Null) | None => Vec::new(),
        Some(other) => {
            return Err(StorageMarketError::Serialization(format!(
                "expected replicas array, found {other:?}"
            )))
        }
    };
    Ok(ContractRecord::with_replicas(contract, replicas))
}

fn storage_contract_to_value(contract: &StorageContract) -> Value {
    let mut map = Map::new();
    map.insert(
        "object_id".into(),
        Value::String(contract.object_id.clone()),
    );
    map.insert(
        "provider_id".into(),
        Value::String(contract.provider_id.clone()),
    );
    map.insert(
        "original_bytes".into(),
        Value::from(contract.original_bytes),
    );
    map.insert("shares".into(), Value::from(contract.shares));
    map.insert(
        "price_per_block".into(),
        Value::from(contract.price_per_block),
    );
    map.insert("start_block".into(), Value::from(contract.start_block));
    map.insert(
        "retention_blocks".into(),
        Value::from(contract.retention_blocks),
    );
    map.insert(
        "next_payment_block".into(),
        Value::from(contract.next_payment_block),
    );
    map.insert("accrued".into(), Value::from(contract.accrued));
    map.insert("total_deposit".into(), Value::from(contract.total_deposit));
    map.insert(
        "last_payment_block".into(),
        contract
            .last_payment_block
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "storage_root".into(),
        Value::Array(
            contract
                .storage_root
                .as_bytes()
                .iter()
                .map(|byte| Value::from(*byte))
                .collect(),
        ),
    );
    Value::Object(map)
}

fn storage_contract_from_value(value: Value) -> Result<StorageContract, StorageMarketError> {
    let mut map = expect_object(value, "storage contract")?;
    let object_id = take_string(&mut map, "object_id", "storage contract")?;
    let provider_id = take_string(&mut map, "provider_id", "storage contract")?;
    let original_bytes = take_u64(&mut map, "original_bytes", "storage contract")?;
    let shares = take_u16(&mut map, "shares", "storage contract")?;
    let price_per_block = take_u64(&mut map, "price_per_block", "storage contract")?;
    let start_block = take_u64(&mut map, "start_block", "storage contract")?;
    let retention_blocks = take_u64(&mut map, "retention_blocks", "storage contract")?;
    let next_payment_block = take_u64(&mut map, "next_payment_block", "storage contract")?;
    let accrued = take_u64(&mut map, "accrued", "storage contract")?;
    let total_deposit = take_u64_default(&mut map, "total_deposit", "storage contract", 0)?;
    let last_payment_block = take_optional_u64(&mut map, "last_payment_block", "storage contract")?;
    let storage_root_bytes = take_storage_root_bytes(&mut map, "storage_root", "storage contract")?;
    Ok(StorageContract {
        object_id,
        provider_id,
        original_bytes,
        shares,
        price_per_block,
        start_block,
        retention_blocks,
        next_payment_block,
        accrued,
        total_deposit,
        last_payment_block,
        storage_root: MerkleRoot::new(storage_root_bytes),
    })
}

fn replica_to_value(replica: &ReplicaIncentive) -> Value {
    let mut map = Map::new();
    map.insert(
        "provider_id".into(),
        Value::String(replica.provider_id.clone()),
    );
    map.insert(
        "allocated_shares".into(),
        Value::from(replica.allocated_shares),
    );
    map.insert(
        "price_per_block".into(),
        Value::from(replica.price_per_block),
    );
    map.insert("deposit".into(), Value::from(replica.deposit));
    map.insert(
        "proof_successes".into(),
        Value::from(replica.proof_successes),
    );
    map.insert("proof_failures".into(), Value::from(replica.proof_failures));
    map.insert(
        "last_proof_block".into(),
        replica
            .last_proof_block
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "last_outcome".into(),
        replica
            .last_outcome
            .as_ref()
            .map(outcome_to_value)
            .unwrap_or(Value::Null),
    );
    Value::Object(map)
}

fn replica_from_value(value: Value) -> Result<ReplicaIncentive, StorageMarketError> {
    let mut map = expect_object(value, "replica")?;
    let provider_id = take_string(&mut map, "provider_id", "replica")?;
    let allocated_shares = take_u16(&mut map, "allocated_shares", "replica")?;
    let price_per_block = take_u64(&mut map, "price_per_block", "replica")?;
    let deposit = take_u64(&mut map, "deposit", "replica")?;
    let proof_successes = take_u64_default(&mut map, "proof_successes", "replica", 0)?;
    let proof_failures = take_u64_default(&mut map, "proof_failures", "replica", 0)?;
    let last_proof_block = take_optional_u64(&mut map, "last_proof_block", "replica")?;
    let last_outcome = take_optional_outcome(&mut map, "last_outcome", "replica")?;
    Ok(ReplicaIncentive {
        provider_id,
        allocated_shares,
        price_per_block,
        deposit,
        proof_successes,
        proof_failures,
        last_proof_block,
        last_outcome,
    })
}

fn outcome_to_value(outcome: &ProofOutcome) -> Value {
    match outcome {
        ProofOutcome::Success => Value::String("Success".to_string()),
        ProofOutcome::Failure => Value::String("Failure".to_string()),
    }
}

fn outcome_from_value(
    value: Value,
    context: &str,
    field: &str,
) -> Result<ProofOutcome, StorageMarketError> {
    match value {
        Value::String(text) => match text.as_str() {
            "Success" => Ok(ProofOutcome::Success),
            "Failure" => Ok(ProofOutcome::Failure),
            other => Err(StorageMarketError::Serialization(format!(
                "unknown proof outcome '{other}' for {context}.{field}"
            ))),
        },
        other => Err(StorageMarketError::Serialization(format!(
            "expected string outcome for {context}.{field}, found {other:?}"
        ))),
    }
}

fn expect_object(value: Value, context: &str) -> Result<Map, StorageMarketError> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(StorageMarketError::Serialization(format!(
            "expected object for {context}, found {other:?}"
        ))),
    }
}

fn field_err(field: &str, context: &str) -> StorageMarketError {
    StorageMarketError::Serialization(format!("missing field '{field}' in {context}"))
}

fn take_storage_root_bytes(
    map: &mut Map,
    field: &str,
    context: &str,
) -> Result<[u8; 32], StorageMarketError> {
    let value = map.remove(field).ok_or_else(|| field_err(field, context))?;
    let array = match value {
        Value::Array(items) => {
            if items.len() != 32 {
                return Err(StorageMarketError::Serialization(format!(
                    "expected 32-byte storage root for {context}.{field}, found length {}",
                    items.len()
                )));
            }
            let mut out = [0u8; 32];
            for (idx, item) in items.into_iter().enumerate() {
                let byte = item.as_u64().ok_or_else(|| {
                    StorageMarketError::Serialization(format!(
                        "storage root byte {idx} for {context}.{field} is not a number"
                    ))
                })?;
                if byte > u8::MAX as u64 {
                    return Err(StorageMarketError::Serialization(format!(
                        "storage root byte {idx} for {context}.{field} out of range: {byte}"
                    )));
                }
                out[idx] = byte as u8;
            }
            out
        }
        other => {
            return Err(StorageMarketError::Serialization(format!(
                "expected array for {context}.{field}, found {other:?}"
            )))
        }
    };
    Ok(array)
}

fn take_string(map: &mut Map, field: &str, context: &str) -> Result<String, StorageMarketError> {
    match map.remove(field) {
        Some(Value::String(value)) => Ok(value),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected string for {context}.{field}, found {other:?}"
        ))),
        None => Err(field_err(field, context)),
    }
}

fn number_as_u64(number: &Number, context: &str, field: &str) -> Result<u64, StorageMarketError> {
    number.as_u64().ok_or_else(|| {
        StorageMarketError::Serialization(format!("invalid unsigned integer for {context}.{field}"))
    })
}

fn take_u64(map: &mut Map, field: &str, context: &str) -> Result<u64, StorageMarketError> {
    match map.remove(field) {
        Some(Value::Number(number)) => number_as_u64(&number, context, field),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected number for {context}.{field}, found {other:?}"
        ))),
        None => Err(field_err(field, context)),
    }
}

fn take_u64_default(
    map: &mut Map,
    field: &str,
    context: &str,
    default: u64,
) -> Result<u64, StorageMarketError> {
    match map.remove(field) {
        Some(Value::Number(number)) => number_as_u64(&number, context, field),
        Some(Value::Null) => Ok(default),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected number for {context}.{field}, found {other:?}"
        ))),
        None => Ok(default),
    }
}

fn take_optional_u64(
    map: &mut Map,
    field: &str,
    context: &str,
) -> Result<Option<u64>, StorageMarketError> {
    match map.remove(field) {
        Some(Value::Number(number)) => Ok(Some(number_as_u64(&number, context, field)?)),
        Some(Value::Null) => Ok(None),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected number or null for {context}.{field}, found {other:?}"
        ))),
        None => Ok(None),
    }
}

fn take_optional_outcome(
    map: &mut Map,
    field: &str,
    context: &str,
) -> Result<Option<ProofOutcome>, StorageMarketError> {
    match map.remove(field) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => outcome_from_value(value, context, field).map(Some),
    }
}

fn take_u16(map: &mut Map, field: &str, context: &str) -> Result<u16, StorageMarketError> {
    let value = take_u64(map, field, context)?;
    value.try_into().map_err(|_| {
        StorageMarketError::Serialization(format!("value for {context}.{field} exceeds u16"))
    })
}

fn take_optional_string(
    map: &mut Map,
    field: &str,
    context: &str,
) -> Result<Option<String>, StorageMarketError> {
    match map.remove(field) {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected string or null for {context}.{field}, found {other:?}"
        ))),
    }
}

fn take_string_array(
    map: &mut Map,
    field: &str,
    context: &str,
) -> Result<Vec<String>, StorageMarketError> {
    match map.remove(field) {
        Some(Value::Array(items)) => {
            let mut strings = Vec::with_capacity(items.len());
            for item in items {
                if let Value::String(value) = item {
                    strings.push(value);
                } else {
                    return Err(StorageMarketError::Serialization(format!(
                        "expected string in {context}.{field} array, found {item:?}"
                    )));
                }
            }
            Ok(strings)
        }
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(StorageMarketError::Serialization(format!(
            "expected array or null for {context}.{field}, found {other:?}"
        ))),
    }
}
