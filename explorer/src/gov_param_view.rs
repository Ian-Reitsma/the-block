use crate::decode_json;
use diagnostics::{Context, Result};
use foundation_serialization::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeeFloorPolicyRecord {
    pub epoch: u64,
    pub proposal_id: u64,
    pub window: i64,
    pub percentile: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyPolicyRecord {
    pub epoch: u64,
    pub proposal_id: u64,
    pub kind: String,
    pub allowed: Vec<String>,
}

fn history_root(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| Path::new(".").to_path_buf())
    }
}

pub fn fee_floor_policy_history(path: impl AsRef<Path>) -> Result<Vec<FeeFloorPolicyRecord>> {
    let base = history_root(path.as_ref());
    let history_file = base.join("governance/history/fee_floor_policy.json");
    if !history_file.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        std::fs::read(&history_file).with_context(|| format!("read {}", history_file.display()))?;
    let mut records: Vec<FeeFloorPolicyRecord> =
        decode_json(&bytes).with_context(|| "decode fee floor policy history".to_string())?;
    records.sort_by_key(|rec| rec.epoch);
    Ok(records)
}

pub fn dependency_policy_history(path: impl AsRef<Path>) -> Result<Vec<DependencyPolicyRecord>> {
    let base = history_root(path.as_ref());
    let history_file = base.join("governance/history/dependency_policy.json");
    if !history_file.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        std::fs::read(&history_file).with_context(|| format!("read {}", history_file.display()))?;
    let mut records: Vec<DependencyPolicyRecord> =
        decode_json(&bytes).with_context(|| "decode dependency policy history".to_string())?;
    records.sort_by_key(|rec| rec.epoch);
    Ok(records)
}
