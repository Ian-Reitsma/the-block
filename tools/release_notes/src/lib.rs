use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};

use diagnostics::anyhow::{anyhow, Context, Result};
use foundation_serialization::json::{self, Map, Number, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let value = json::value_from_slice(&bytes).with_context(|| {
        format!(
            "failed to decode dependency policy history from {}",
            path.display()
        )
    })?;
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("dependency policy history must be a JSON array"))?;

    let mut records = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let record =
            parse_record(item).with_context(|| format!("invalid record at index {idx}"))?;
        records.push(record);
    }

    Ok(records)
}

fn parse_record(value: &Value) -> Result<DependencyPolicyRecord> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("dependency policy entry must be a JSON object"))?;

    Ok(DependencyPolicyRecord {
        epoch: require_u64(object, "epoch")?,
        proposal_id: require_u64(object, "proposal_id")?,
        kind: require_string(object, "kind")?,
        allowed: require_string_array(object, "allowed")?,
    })
}

fn require_u64(object: &Map, field: &str) -> Result<u64> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("field '{field}' must be an unsigned integer"))
}

fn require_string(object: &Map, field: &str) -> Result<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("field '{field}' must be a string"))
}

fn require_string_array(object: &Map, field: &str) -> Result<Vec<String>> {
    let array = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("field '{field}' must be an array"))?;

    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow!("array '{field}' must contain only strings"))
        })
        .collect()
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

pub fn summary_to_value(summary: &Summary) -> Value {
    let updates = summary
        .updates
        .iter()
        .map(|update| {
            let mut map = Map::new();
            map.insert("kind".into(), Value::String(update.kind.clone()));
            map.insert("epoch".into(), Value::Number(Number::from(update.epoch)));
            map.insert(
                "proposal_id".into(),
                Value::Number(Number::from(update.proposal_id)),
            );
            map.insert(
                "current".into(),
                Value::Array(string_list_to_values(&update.current)),
            );
            map.insert(
                "added".into(),
                Value::Array(string_list_to_values(&update.added)),
            );
            map.insert(
                "removed".into(),
                Value::Array(string_list_to_values(&update.removed)),
            );
            map.insert(
                "previous".into(),
                update
                    .previous
                    .as_ref()
                    .map(|values| Value::Array(string_list_to_values(values)))
                    .unwrap_or(Value::Null),
            );
            Value::Object(map)
        })
        .collect::<Vec<_>>();

    let mut latest_map = Map::new();
    for (kind, policy) in &summary.latest {
        let mut map = Map::new();
        map.insert(
            "allowed".into(),
            Value::Array(string_list_to_values(&policy.allowed)),
        );
        map.insert("epoch".into(), Value::Number(Number::from(policy.epoch)));
        map.insert(
            "proposal_id".into(),
            Value::Number(Number::from(policy.proposal_id)),
        );
        latest_map.insert(kind.clone(), Value::Object(map));
    }

    let mut root = Map::new();
    root.insert("updates".into(), Value::Array(updates));
    root.insert("latest".into(), Value::Object(latest_map));
    Value::Object(root)
}

fn string_list_to_values(values: &[String]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String(value.clone()))
        .collect()
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
            record("runtime_backend", 10, 1, &["inhouse"]),
            record("transport_provider", 12, 2, &["quinn"]),
            record("runtime_backend", 15, 3, &["inhouse", "stub"]),
            record("storage_engine", 20, 3, &["inhouse", "rocksdb-compat"]),
        ];

        let summary = summarise(&records, Filter::default());
        assert_eq!(summary.updates.len(), 4);
        assert_eq!(summary.updates[2].kind, "runtime_backend");
        assert_eq!(summary.updates[2].epoch, 15);
        assert_eq!(summary.updates[2].proposal_id, 3);
        assert_eq!(
            summary.updates[2].previous,
            Some(vec!["inhouse".to_string()])
        );
        assert_eq!(
            summary.updates[2].current,
            vec!["inhouse".to_string(), "stub".to_string()]
        );
        assert_eq!(summary.updates[2].added, vec!["stub".to_string()]);
        assert!(summary.updates[2].removed.is_empty());

        let runtime = summary
            .latest
            .get("runtime_backend")
            .expect("runtime backend latest");
        assert_eq!(
            runtime.allowed,
            vec!["inhouse".to_string(), "stub".to_string()]
        );
        assert_eq!(runtime.epoch, 15);
        assert_eq!(runtime.proposal_id, 3);
    }

    #[test]
    fn filter_excludes_earlier_entries() {
        let records = vec![
            record("runtime_backend", 10, 1, &["inhouse"]),
            record("runtime_backend", 11, 2, &["inhouse", "stub"]),
            record("storage_engine", 12, 3, &["rocksdb-compat"]),
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
        assert_eq!(
            summary.updates[0].previous,
            Some(vec!["inhouse".to_string()])
        );
        assert_eq!(summary.updates[0].added, vec!["stub".to_string()]);
        assert!(summary.updates[0].removed.is_empty());
        assert_eq!(summary.updates[1].kind, "storage_engine");
    }

    #[test]
    fn tracks_added_and_removed_backends() {
        let records = vec![
            record("runtime_backend", 1, 1, &["inhouse", "smol"]),
            record("runtime_backend", 2, 2, &["smol"]),
            record("runtime_backend", 3, 3, &["smol", "stub"]),
        ];

        let summary = summarise(&records, Filter::default());
        assert_eq!(summary.updates.len(), 3);

        let removal = &summary.updates[1];
        assert_eq!(removal.removed, vec!["inhouse".to_string()]);
        assert!(removal.added.is_empty());

        let addition = &summary.updates[2];
        assert_eq!(addition.added, vec!["stub".to_string()]);
        assert!(addition.removed.is_empty());
    }

    #[test]
    fn change_summary_formats_human_readable_delta() {
        let added = vec!["inhouse".to_string(), "smol".to_string()];
        let removed = vec!["async-std".to_string()];
        let summary = format_change_summary(&added, &removed).expect("delta formatted");
        assert_eq!(summary, "added inhouse, smol; removed async-std");

        assert!(format_change_summary(&[], &[]).is_none());
        assert_eq!(
            format_change_summary(&added, &[]).unwrap(),
            "added inhouse, smol"
        );
    }

    #[test]
    fn summary_serialises_to_json_value() {
        let records = vec![
            record("runtime_backend", 1, 1, &["inhouse"]),
            record("runtime_backend", 2, 2, &["inhouse", "stub"]),
        ];
        let summary = summarise(&records, Filter::default());
        let value = summary_to_value(&summary);
        let object = value.as_object().expect("summary root object");
        assert!(object.contains_key("updates"));
        assert!(object.contains_key("latest"));
        let updates = object
            .get("updates")
            .and_then(|v| v.as_array())
            .expect("updates array");
        assert_eq!(updates.len(), 2);
        let first = updates[0]
            .as_object()
            .expect("update object contains fields");
        assert_eq!(
            first.get("kind").and_then(Value::as_str),
            Some("runtime_backend")
        );
        assert!(first.contains_key("added"));
        let latest = object
            .get("latest")
            .and_then(|v| v.as_object())
            .expect("latest map");
        assert!(latest.contains_key("runtime_backend"));
    }
}
