#![forbid(unsafe_code)]

use crate::codec::{deserialize_contract_record, serialize_contract_record};
use crate::{ContractRecord, Result, StorageMarketError};
use crypto_suite::hex;
use foundation_serialization::json::{self, Value};
use std::fs;
use std::path::{Path, PathBuf};

use super::engine::Tree;

const LEGACY_MANIFEST: &str = "legacy_manifest.json";
pub(crate) const LEGACY_TREE_HEX: &str = "6d61726b65742f636f6e747261637473"; // "market/contracts"
const MIGRATED_SUFFIX: &str = "legacy_manifest.migrated";

pub fn migrate_if_present(base: &Path, contracts: &Tree) -> Result<()> {
    let manifest_path = base.join(LEGACY_MANIFEST);
    if !manifest_path.exists() {
        return Ok(());
    }

    let entries = load_legacy_contracts(&manifest_path)?;
    if entries.is_empty() {
        mark_migrated(&manifest_path)?;
        return Ok(());
    }

    for (key, record) in entries {
        if contracts.get(&key)?.is_some() {
            continue;
        }
        let value = serialize_contract_record(&record)?;
        let _ = contracts.insert(&key, &value)?;
    }

    mark_migrated(&manifest_path)?;
    Ok(())
}

fn load_legacy_contracts(path: &Path) -> Result<Vec<(Vec<u8>, ContractRecord)>> {
    let bytes = fs::read(path).map_err(|err| {
        StorageMarketError::LegacyManifest(format!(
            "failed to read legacy manifest {}: {err}",
            display_path(path)
        ))
    })?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = json::value_from_slice(&bytes).map_err(|err| {
        StorageMarketError::LegacyManifest(format!(
            "failed to parse legacy manifest {}: {err}",
            display_path(path)
        ))
    })?;
    let root = value.as_object().ok_or_else(|| {
        StorageMarketError::LegacyManifest(format!(
            "legacy manifest {} must contain a root object",
            display_path(path)
        ))
    })?;
    let trees = root
        .get("trees")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            StorageMarketError::LegacyManifest(format!(
                "legacy manifest {} missing 'trees' map",
                display_path(path)
            ))
        })?;

    let legacy_tree = trees.get(LEGACY_TREE_HEX).and_then(Value::as_array);
    let Some(entries) = legacy_tree else {
        return Ok(Vec::new());
    };

    let mut records = Vec::with_capacity(entries.len());
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            StorageMarketError::LegacyManifest("legacy manifest entry must be an object".into())
        })?;
        let key_hex = object.get("key").and_then(Value::as_str).ok_or_else(|| {
            StorageMarketError::LegacyManifest("legacy manifest entry missing 'key' field".into())
        })?;
        let value_hex = object.get("value").and_then(Value::as_str).ok_or_else(|| {
            StorageMarketError::LegacyManifest("legacy manifest entry missing 'value' field".into())
        })?;
        let key = hex::decode(key_hex.as_bytes()).map_err(|_| {
            StorageMarketError::LegacyManifest("legacy manifest entry has invalid key hex".into())
        })?;
        let legacy_bytes = hex::decode(value_hex.as_bytes()).map_err(|_| {
            StorageMarketError::LegacyManifest("legacy manifest entry has invalid value hex".into())
        })?;
        let record = deserialize_contract_record(&legacy_bytes)?;
        records.push((key, record));
    }
    Ok(records)
}

fn mark_migrated(path: &Path) -> Result<()> {
    let target = migrated_path(path);
    fs::rename(path, target).map_err(|err| {
        StorageMarketError::LegacyManifest(format!(
            "failed to rename legacy manifest {}: {err}",
            display_path(path)
        ))
    })?;
    Ok(())
}

fn migrated_path(path: &Path) -> PathBuf {
    let mut target = path.to_path_buf();
    target.set_file_name(format!("{MIGRATED_SUFFIX}.json"));
    target
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
