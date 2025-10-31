#![forbid(unsafe_code)]

use foundation_serialization::json::{self, Map, Number, Value};

use crate::{ContractRecord, ProofOutcome, ReplicaIncentive, StorageMarketError};
use storage::StorageContract;

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
    map.insert(
        "total_deposit_ct".into(),
        Value::from(contract.total_deposit_ct),
    );
    map.insert(
        "last_payment_block".into(),
        contract
            .last_payment_block
            .map(Value::from)
            .unwrap_or(Value::Null),
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
    let total_deposit_ct = take_u64_default(&mut map, "total_deposit_ct", "storage contract", 0)?;
    let last_payment_block = take_optional_u64(&mut map, "last_payment_block", "storage contract")?;
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
        total_deposit_ct,
        last_payment_block,
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
    map.insert("deposit_ct".into(), Value::from(replica.deposit_ct));
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
    let deposit_ct = take_u64(&mut map, "deposit_ct", "replica")?;
    let proof_successes = take_u64_default(&mut map, "proof_successes", "replica", 0)?;
    let proof_failures = take_u64_default(&mut map, "proof_failures", "replica", 0)?;
    let last_proof_block = take_optional_u64(&mut map, "last_proof_block", "replica")?;
    let last_outcome = take_optional_outcome(&mut map, "last_outcome", "replica")?;
    Ok(ReplicaIncentive {
        provider_id,
        allocated_shares,
        price_per_block,
        deposit_ct,
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
