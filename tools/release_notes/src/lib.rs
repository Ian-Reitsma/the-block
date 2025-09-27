use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DependencyPolicyRecord {
    pub epoch: u64,
    pub proposal_id: u64,
    pub kind: String,
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Filter {
    pub since_epoch: Option<u64>,
    pub since_proposal: Option<u64>,
}

impl Filter {
    pub fn allows(&self, record: &DependencyPolicyRecord) -> bool {
        if let Some(epoch) = self.since_epoch {
            if record.epoch < epoch {
                return false;
            }
        }
        if let Some(proposal) = self.since_proposal {
            if record.proposal_id < proposal {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyUpdate {
    pub kind: String,
    pub epoch: u64,
    pub proposal_id: u64,
    pub previous: Option<Vec<String>>,
    pub current: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestPolicy {
    pub allowed: Vec<String>,
    pub epoch: u64,
    pub proposal_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub updates: Vec<PolicyUpdate>,
    pub latest: BTreeMap<String, LatestPolicy>,
}

pub const KNOWN_KINDS: [&str; 3] = ["runtime_backend", "transport_provider", "storage_engine"];

pub fn load_history(path: &Path) -> Result<Vec<DependencyPolicyRecord>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "failed to decode dependency policy history from {}",
            path.display()
        )
    })
}

pub fn summarise(records: &[DependencyPolicyRecord], filter: Filter) -> Summary {
    let mut sorted = records.to_vec();
    sorted.sort_by(|a, b| {
        a.epoch
            .cmp(&b.epoch)
            .then_with(|| a.proposal_id.cmp(&b.proposal_id))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    let mut latest: BTreeMap<String, LatestPolicy> = BTreeMap::new();
    let mut prior: BTreeMap<String, LatestPolicy> = BTreeMap::new();
    let mut updates = Vec::new();

    for record in sorted {
        let current_allowed = normalise_allowed(&record.allowed);
        let previous = prior.get(&record.kind).cloned();
        let previous_allowed = previous
            .as_ref()
            .map(|policy| policy.allowed.clone())
            .unwrap_or_default();

        let (added, removed) = diff_allowed(&current_allowed, &previous_allowed);

        if filter.allows(&record) {
            updates.push(PolicyUpdate {
                kind: record.kind.clone(),
                epoch: record.epoch,
                proposal_id: record.proposal_id,
                previous: previous.as_ref().map(|policy| policy.allowed.clone()),
                current: current_allowed.clone(),
                added: added.clone(),
                removed: removed.clone(),
            });
        }

        let latest_policy = LatestPolicy {
            allowed: current_allowed.clone(),
            epoch: record.epoch,
            proposal_id: record.proposal_id,
        };
        prior.insert(record.kind.clone(), latest_policy.clone());
        latest.insert(record.kind.clone(), latest_policy);
    }

    Summary { updates, latest }
}

fn normalise_allowed(values: &[String]) -> Vec<String> {
    use std::collections::BTreeSet;

    let set: BTreeSet<String> = values.iter().map(|value| value.to_string()).collect();
    set.into_iter().collect()
}

fn diff_allowed(current: &[String], previous: &[String]) -> (Vec<String>, Vec<String>) {
    use std::collections::BTreeSet;

    let current_set: BTreeSet<_> = current.iter().cloned().collect();
    let previous_set: BTreeSet<_> = previous.iter().cloned().collect();

    let added = current_set
        .difference(&previous_set)
        .cloned()
        .collect::<Vec<_>>();
    let removed = previous_set
        .difference(&current_set)
        .cloned()
        .collect::<Vec<_>>();

    (added, removed)
}

pub fn kind_label(kind: &str) -> Cow<'static, str> {
    match kind {
        "runtime_backend" => Cow::Borrowed("Runtime backend"),
        "transport_provider" => Cow::Borrowed("Transport provider"),
        "storage_engine" => Cow::Borrowed("Storage engine"),
        other => Cow::Owned(other.to_string()),
    }
}

pub fn format_allowed(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else if values.len() == 1 {
        values[0].clone()
    } else {
        values.join(", ")
    }
}

pub fn format_change_summary(added: &[String], removed: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!("added {}", format_allowed(added)));
    }
    if !removed.is_empty() {
        parts.push(format!("removed {}", format_allowed(removed)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        kind: &str,
        epoch: u64,
        proposal_id: u64,
        allowed: &[&str],
    ) -> DependencyPolicyRecord {
        DependencyPolicyRecord {
            epoch,
            proposal_id,
            kind: kind.to_string(),
            allowed: allowed.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn summary_tracks_updates_and_latest() {
        let records = vec![
            record("runtime_backend", 10, 1, &["tokio"]),
            record("transport_provider", 12, 2, &["quinn"]),
            record("runtime_backend", 15, 3, &["tokio", "stub"]),
            record("storage_engine", 20, 4, &["rocksdb", "sled"]),
        ];

        let summary = summarise(&records, Filter::default());
        assert_eq!(summary.updates.len(), 4);
        assert_eq!(summary.updates[2].kind, "runtime_backend");
        assert_eq!(summary.updates[2].epoch, 15);
        assert_eq!(summary.updates[2].proposal_id, 3);
        assert_eq!(summary.updates[2].previous, Some(vec!["tokio".to_string()]));
        assert_eq!(
            summary.updates[2].current,
            vec!["stub".to_string(), "tokio".to_string()]
        );
        assert_eq!(summary.updates[2].added, vec!["stub".to_string()]);
        assert!(summary.updates[2].removed.is_empty());

        let runtime = summary
            .latest
            .get("runtime_backend")
            .expect("runtime backend latest");
        assert_eq!(
            runtime.allowed,
            vec!["stub".to_string(), "tokio".to_string()]
        );
        assert_eq!(runtime.epoch, 15);
        assert_eq!(runtime.proposal_id, 3);
    }

    #[test]
    fn filter_excludes_earlier_entries() {
        let records = vec![
            record("runtime_backend", 10, 1, &["tokio"]),
            record("runtime_backend", 11, 2, &["tokio", "stub"]),
            record("storage_engine", 12, 3, &["rocksdb"]),
        ];

        let summary = summarise(
            &records,
            Filter {
                since_epoch: Some(11),
                since_proposal: None,
            },
        );
        assert_eq!(summary.updates.len(), 2);
        assert_eq!(summary.updates[0].epoch, 11);
        assert_eq!(summary.updates[0].previous, Some(vec!["tokio".to_string()]));
        assert_eq!(summary.updates[0].added, vec!["stub".to_string()]);
        assert!(summary.updates[0].removed.is_empty());
        assert_eq!(summary.updates[1].kind, "storage_engine");
    }

    #[test]
    fn tracks_added_and_removed_backends() {
        let records = vec![
            record("runtime_backend", 1, 1, &["tokio", "smol"]),
            record("runtime_backend", 2, 2, &["smol"]),
            record("runtime_backend", 3, 3, &["smol", "glommio"]),
        ];

        let summary = summarise(&records, Filter::default());
        assert_eq!(summary.updates.len(), 3);

        let removal = &summary.updates[1];
        assert_eq!(removal.removed, vec!["tokio".to_string()]);
        assert!(removal.added.is_empty());

        let addition = &summary.updates[2];
        assert_eq!(addition.added, vec!["glommio".to_string()]);
        assert!(addition.removed.is_empty());
    }

    #[test]
    fn change_summary_formats_human_readable_delta() {
        let added = vec!["smol".to_string(), "tokio".to_string()];
        let removed = vec!["async-std".to_string()];
        let summary = format_change_summary(&added, &removed).expect("delta formatted");
        assert_eq!(summary, "added smol, tokio; removed async-std");

        assert!(format_change_summary(&[], &[]).is_none());
        assert_eq!(
            format_change_summary(&added, &[]).unwrap(),
            "added smol, tokio"
        );
    }
}
