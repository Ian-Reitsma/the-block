#![forbid(unsafe_code)]

use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use storage::StorageContract;
use storage_market::{ContractRecord, ReplicaIncentive, LEGACY_MANIFEST_FILE};
use sys::tempfile::tempdir;

type TestResult<T> = Result<T, Box<dyn Error>>;

fn write_manifest(dir: &Path, records: &[ContractRecord]) -> TestResult<PathBuf> {
    let mut trees = JsonMap::new();
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let key_bytes = record.contract.object_id.as_bytes();
        let mut entry = JsonMap::new();
        entry.insert(
            "key".into(),
            JsonValue::String(crypto_suite::hex::encode(key_bytes)),
        );
        let value = contract_record_to_value(record);
        entry.insert(
            "value".into(),
            JsonValue::String(crypto_suite::hex::encode(&json::to_vec_value(&value))),
        );
        entries.push(JsonValue::Object(entry));
    }
    trees.insert(
        crypto_suite::hex::encode("market/contracts".as_bytes()),
        JsonValue::Array(entries),
    );
    let mut root = JsonMap::new();
    root.insert("trees".into(), JsonValue::Object(trees));
    let bytes = json::to_vec_value(&JsonValue::Object(root));
    fs::create_dir_all(dir)?;
    let path = dir.join(LEGACY_MANIFEST_FILE);
    fs::write(&path, bytes)?;
    Ok(path)
}

fn contract_record_to_value(record: &ContractRecord) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "contract".into(),
        storage_contract_to_value(&record.contract),
    );
    let replicas = record.replicas.iter().map(replica_to_value).collect();
    map.insert("replicas".into(), JsonValue::Array(replicas));
    JsonValue::Object(map)
}

fn storage_contract_to_value(contract: &StorageContract) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "object_id".into(),
        JsonValue::String(contract.object_id.clone()),
    );
    map.insert(
        "provider_id".into(),
        JsonValue::String(contract.provider_id.clone()),
    );
    map.insert(
        "original_bytes".into(),
        JsonValue::Number(JsonNumber::from(contract.original_bytes)),
    );
    map.insert(
        "shares".into(),
        JsonValue::Number(JsonNumber::from(contract.shares)),
    );
    map.insert(
        "price_per_block".into(),
        JsonValue::Number(JsonNumber::from(contract.price_per_block)),
    );
    map.insert(
        "start_block".into(),
        JsonValue::Number(JsonNumber::from(contract.start_block)),
    );
    map.insert(
        "retention_blocks".into(),
        JsonValue::Number(JsonNumber::from(contract.retention_blocks)),
    );
    map.insert(
        "next_payment_block".into(),
        JsonValue::Number(JsonNumber::from(contract.next_payment_block)),
    );
    map.insert(
        "accrued".into(),
        JsonValue::Number(JsonNumber::from(contract.accrued)),
    );
    map.insert(
        "total_deposit_ct".into(),
        JsonValue::Number(JsonNumber::from(contract.total_deposit_ct)),
    );
    map.insert(
        "last_payment_block".into(),
        contract
            .last_payment_block
            .map(|block| JsonValue::Number(JsonNumber::from(block)))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(map)
}

fn replica_to_value(replica: &ReplicaIncentive) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "provider_id".into(),
        JsonValue::String(replica.provider_id.clone()),
    );
    map.insert(
        "allocated_shares".into(),
        JsonValue::Number(JsonNumber::from(replica.allocated_shares)),
    );
    map.insert(
        "price_per_block".into(),
        JsonValue::Number(JsonNumber::from(replica.price_per_block)),
    );
    map.insert(
        "deposit_ct".into(),
        JsonValue::Number(JsonNumber::from(replica.deposit_ct)),
    );
    map.insert(
        "proof_successes".into(),
        JsonValue::Number(JsonNumber::from(replica.proof_successes)),
    );
    map.insert(
        "proof_failures".into(),
        JsonValue::Number(JsonNumber::from(replica.proof_failures)),
    );
    map.insert(
        "last_proof_block".into(),
        replica
            .last_proof_block
            .map(|block| JsonValue::Number(JsonNumber::from(block)))
            .unwrap_or(JsonValue::Null),
    );
    map.insert(
        "last_outcome".into(),
        replica
            .last_outcome
            .as_ref()
            .map(|outcome| match outcome {
                storage_market::ProofOutcome::Success => "Success",
                storage_market::ProofOutcome::Failure => "Failure",
            })
            .map(|label| JsonValue::String(label.into()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(map)
}

fn sample_contract(object_id: &str, provider: &str) -> ContractRecord {
    let contract = StorageContract {
        object_id: object_id.into(),
        provider_id: provider.into(),
        original_bytes: 1024,
        shares: 2,
        price_per_block: 5,
        start_block: 0,
        retention_blocks: 8,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 0,
        last_payment_block: None,
    };
    let replicas = vec![ReplicaIncentive::new(provider.into(), 2, 5, 10)];
    ContractRecord::with_replicas(contract, replicas)
}

fn run_cli(args: &[&str]) -> TestResult<(JsonValue, String)> {
    let cli = env!("CARGO_BIN_EXE_contract-cli");
    let output = Command::new(cli).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let json = json::value_from_slice(&output.stdout)?;
    Ok((json, String::from_utf8_lossy(&output.stdout).to_string()))
}

#[test]
fn storage_importer_cli_flow_reports_json() -> TestResult<()> {
    let dir = tempdir()?;
    let base = dir.path().join("market");
    let records = vec![
        sample_contract("obj-a", "prov-a"),
        sample_contract("obj-b", "prov-b"),
    ];
    write_manifest(&base, &records)?;

    let dir_str = base.to_string_lossy().into_owned();
    let audit_out_path = base.join("audit.json");
    let audit_out = audit_out_path.to_string_lossy().into_owned();
    let (audit_json, _) = run_cli(&[
        "storage", "importer", "audit", "--dir", &dir_str, "--json", "--out", &audit_out,
    ])?;

    assert_eq!(
        audit_json
            .get("summary")
            .and_then(|summary| summary.get("total_entries"))
            .and_then(JsonValue::as_u64),
        Some(records.len() as u64)
    );

    let written = fs::read(&audit_out_path)?;
    let audit_file: JsonValue = json::value_from_slice(&written)?;
    assert_eq!(audit_file, audit_json);

    let (dry_json, _) = run_cli(&[
        "storage",
        "importer",
        "rerun",
        "--dir",
        &dir_str,
        "--json",
        "--dry-run",
    ])?;
    assert!(dry_json.get("result").is_none());
    assert_eq!(
        dry_json
            .get("summary")
            .and_then(|summary| summary.get("missing"))
            .and_then(JsonValue::as_u64),
        Some(records.len() as u64)
    );

    let (run_json, _) = run_cli(&["storage", "importer", "rerun", "--dir", &dir_str, "--json"])?;
    let result = run_json
        .get("result")
        .and_then(JsonValue::as_object)
        .expect("import result present");
    assert_eq!(
        result.get("applied").and_then(JsonValue::as_u64),
        Some(records.len() as u64)
    );
    assert_eq!(result.get("no_change").and_then(JsonValue::as_u64), Some(0));

    let (verify_json, _) =
        run_cli(&["storage", "importer", "verify", "--dir", &dir_str, "--json"])?;
    assert_eq!(
        verify_json.get("matches").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        verify_json
            .get("database")
            .and_then(|db| db.get("entries"))
            .and_then(JsonValue::as_u64),
        Some(records.len() as u64)
    );

    // A second rerun should emit skipped stats without modifying state.
    let (repeat_json, _) = run_cli(&["storage", "importer", "rerun", "--dir", &dir_str, "--json"])?;
    let repeat_result = repeat_json
        .get("result")
        .and_then(JsonValue::as_object)
        .expect("repeat result present");
    assert_eq!(
        repeat_result
            .get("skipped_existing")
            .and_then(JsonValue::as_u64),
        Some(records.len() as u64)
    );

    Ok(())
}
