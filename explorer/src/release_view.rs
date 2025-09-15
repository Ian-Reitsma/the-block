use anyhow::Result;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use the_block::governance::{self, ApprovedRelease, GovStore};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseHistoryEntry {
    pub build_hash: String,
    pub proposer: String,
    pub activated_epoch: u64,
    pub last_install_ts: Option<u64>,
}

fn to_entry(record: ApprovedRelease, installs: &mut HashMap<String, u64>) -> ReleaseHistoryEntry {
    let last_install_ts = installs.remove(&record.build_hash);
    ReleaseHistoryEntry {
        build_hash: record.build_hash,
        proposer: record.proposer,
        activated_epoch: record.activated_epoch,
        last_install_ts,
    }
}

pub fn release_history(path: impl AsRef<Path>) -> Result<Vec<ReleaseHistoryEntry>> {
    let store = GovStore::open(path);
    let mut install_map: HashMap<String, u64> =
        governance::controller::release_installations(&store)?
            .into_iter()
            .collect();
    let mut entries: Vec<ReleaseHistoryEntry> = governance::controller::approved_releases(&store)?
        .into_iter()
        .map(|record| to_entry(record, &mut install_map))
        .collect();
    entries.sort_by_key(|entry| Reverse(entry.activated_epoch));
    Ok(entries)
}
