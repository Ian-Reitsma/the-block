#![forbid(unsafe_code)]

use foundation_serialization::json::{self, Map, Value};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MemorySnapshotEntry {
    pub latest: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessTelemetry {
    pub ready: bool,
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    pub unique_viewers: u64,
    pub host_count: u64,
    pub provider_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub blockers: Vec<String>,
    pub last_updated: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub total_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub settlement_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub ct_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub it_price_usd_micros: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct WrapperMetricEntry {
    pub metric: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct WrapperSummaryEntry {
    pub metrics: Vec<WrapperMetricEntry>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TelemetrySummary {
    pub node_id: String,
    pub seq: u64,
    pub timestamp: u64,
    pub sample_rate_ppm: u64,
    pub compaction_secs: u64,
    pub memory: HashMap<String, MemorySnapshotEntry>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub wrappers: WrapperSummaryEntry,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub ad_readiness: Option<AdReadinessTelemetry>,
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    path: String,
    message: String,
}

impl ValidationError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

impl TelemetrySummary {
    pub fn validate_value(value: &Value) -> Result<(), ValidationError> {
        let root = expect_object(value, "/", "telemetry summary")?;

        let node_id_path = child_path("/", "node_id");
        let node_id = expect_string_field(root, "node_id", &node_id_path)?;
        if node_id.trim().is_empty() {
            return Err(ValidationError::new(
                node_id_path,
                "node_id must not be empty",
            ));
        }

        expect_u64_field(root, "seq", &child_path("/", "seq"))?;
        expect_u64_field(root, "timestamp", &child_path("/", "timestamp"))?;
        expect_u64_field(root, "sample_rate_ppm", &child_path("/", "sample_rate_ppm"))?;
        expect_u64_field(root, "compaction_secs", &child_path("/", "compaction_secs"))?;

        let memory_value = root
            .get("memory")
            .ok_or_else(|| ValidationError::new(child_path("/", "memory"), "missing field"))?;
        let memory_path = child_path("/", "memory");
        let memory = expect_object(memory_value, &memory_path, "memory summary")?;
        for (bucket, entry) in memory {
            let entry_path = child_path(&memory_path, bucket);
            let entry_obj = expect_object(entry, &entry_path, "memory entry")?;
            for field in ["latest", "p50", "p90", "p99"] {
                let field_path = child_path(&entry_path, field);
                expect_u64_field(entry_obj, field, &field_path)?;
            }
        }

        if let Some(wrappers_value) = root.get("wrappers") {
            let wrappers_path = child_path("/", "wrappers");
            let wrappers = expect_object(wrappers_value, &wrappers_path, "wrappers summary")?;
            let metrics_value = wrappers.get("metrics").ok_or_else(|| {
                ValidationError::new(child_path(&wrappers_path, "metrics"), "missing field")
            })?;
            let metrics_array = metrics_value.as_array().ok_or_else(|| {
                ValidationError::new(
                    child_path(&wrappers_path, "metrics"),
                    "metrics must be an array",
                )
            })?;
            for (idx, metric_value) in metrics_array.iter().enumerate() {
                let metric_path = format!("{}/metrics[{}]", wrappers_path, idx);
                let metric_obj = expect_object(metric_value, &metric_path, "wrapper metric")?;
                let metric_name =
                    expect_string_field(metric_obj, "metric", &child_path(&metric_path, "metric"))?;
                if metric_name.trim().is_empty() {
                    return Err(ValidationError::new(
                        child_path(&metric_path, "metric"),
                        "metric name must not be empty",
                    ));
                }
                if let Some(labels_value) = metric_obj.get("labels") {
                    let labels_path = child_path(&metric_path, "labels");
                    let labels = expect_object(labels_value, &labels_path, "metric labels")?;
                    for (label_key, label_value) in labels {
                        let label_path = child_path(&labels_path, label_key);
                        expect_string(label_value, &label_path, "label value")?;
                    }
                }
                let value_field_path = child_path(&metric_path, "value");
                expect_f64_field(metric_obj, "value", &value_field_path)?;
            }
        }

        if let Some(readiness_value) = root.get("ad_readiness") {
            if !matches!(readiness_value, Value::Null) {
                let readiness_path = child_path("/", "ad_readiness");
                let readiness =
                    expect_object(readiness_value, &readiness_path, "ad readiness summary")?;
                let ready_path = child_path(&readiness_path, "ready");
                readiness
                    .get("ready")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| ValidationError::new(ready_path, "ready must be a boolean"))?;
                for field in [
                    "window_secs",
                    "min_unique_viewers",
                    "min_host_count",
                    "min_provider_count",
                    "unique_viewers",
                    "host_count",
                    "provider_count",
                    "last_updated",
                    "total_usd_micros",
                    "settlement_count",
                    "ct_price_usd_micros",
                    "it_price_usd_micros",
                ] {
                    let field_path = child_path(&readiness_path, field);
                    expect_u64_field(readiness, field, &field_path)?;
                }
                if let Some(blockers_value) = readiness.get("blockers") {
                    let blockers_path = child_path(&readiness_path, "blockers");
                    let blockers = blockers_value.as_array().ok_or_else(|| {
                        ValidationError::new(blockers_path.clone(), "blockers must be an array")
                    })?;
                    for (idx, blocker) in blockers.iter().enumerate() {
                        let path = format!("{}/{}", blockers_path, idx);
                        expect_string(blocker, &path, "blocker")?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn from_value(value: Value) -> Result<Self, ValidationError> {
        Self::validate_value(&value)?;
        json::from_value(value).map_err(|err| {
            ValidationError::new(
                "/",
                format!("failed to deserialize telemetry summary: {err}"),
            )
        })
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        let value: Value = json::from_slice(bytes).map_err(|err| {
            ValidationError::new("/", format!("failed to parse telemetry summary: {err}"))
        })?;
        Self::from_value(value)
    }
}

fn expect_object<'a>(
    value: &'a Value,
    path: &str,
    label: &str,
) -> Result<&'a Map, ValidationError> {
    value.as_object().ok_or_else(|| {
        ValidationError::new(path.to_string(), format!("{label} must be a JSON object"))
    })
}

fn expect_string_field<'a>(
    map: &'a Map,
    key: &str,
    path: &str,
) -> Result<&'a str, ValidationError> {
    let value = map
        .get(key)
        .ok_or_else(|| ValidationError::new(path, "missing field"))?;
    expect_string(value, path, "string value")
}

fn expect_string<'a>(
    value: &'a Value,
    path: &str,
    label: &str,
) -> Result<&'a str, ValidationError> {
    value
        .as_str()
        .ok_or_else(|| ValidationError::new(path.to_string(), format!("{label} must be a string")))
}

fn expect_u64_field(map: &Map, key: &str, path: &str) -> Result<u64, ValidationError> {
    let value = map
        .get(key)
        .ok_or_else(|| ValidationError::new(path, "missing field"))?;
    value.as_u64().ok_or_else(|| {
        ValidationError::new(path.to_string(), "value must be a non-negative integer")
    })
}

fn expect_f64_field(map: &Map, key: &str, path: &str) -> Result<f64, ValidationError> {
    let value = map
        .get(key)
        .ok_or_else(|| ValidationError::new(path, "missing field"))?;
    value
        .as_f64()
        .ok_or_else(|| ValidationError::new(path.to_string(), "value must be a number"))
}

fn child_path(parent: &str, key: &str) -> String {
    if parent == "/" {
        format!("/{key}")
    } else {
        format!("{parent}/{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary() -> Value {
        json::value_from_str(
            r#"{
                "node_id": "node-a",
                "seq": 1,
                "timestamp": 1700000000,
                "sample_rate_ppm": 500000,
                "compaction_secs": 30,
                "memory": {
                    "mempool": {"latest": 1024, "p50": 800, "p90": 900, "p99": 1000}
                },
                "wrappers": {
                    "metrics": [
                        {"metric": "foo", "labels": {"region": "us"}, "value": 1.0}
                    ]
                }
            }"#,
        )
        .expect("valid summary json")
    }

    #[test]
    fn validates_sample_summary() {
        let value = sample_summary();
        TelemetrySummary::validate_value(&value).expect("summary should validate");
        let summary = TelemetrySummary::from_value(value).expect("summary should parse");
        assert_eq!(summary.node_id, "node-a");
    }

    #[test]
    fn detects_missing_memory() {
        let mut value = sample_summary();
        if let Value::Object(ref mut map) = value {
            map.remove("memory");
        }
        let err = TelemetrySummary::validate_value(&value).expect_err("memory missing should fail");
        assert!(err.path().contains("/memory"));
    }

    #[test]
    fn detects_invalid_wrapper_label_type() {
        let mut value = sample_summary();
        if let Value::Object(ref mut root) = value {
            if let Some(Value::Object(wrappers)) = root.get_mut("wrappers") {
                if let Some(Value::Array(metrics)) = wrappers.get_mut("metrics") {
                    if let Some(Value::Object(metric)) = metrics.first_mut() {
                        metric.insert("labels".into(), Value::from(42));
                    }
                }
            }
        }
        let err = TelemetrySummary::validate_value(&value).expect_err("labels must be map");
        assert!(err.message().contains("labels"));
    }
}
