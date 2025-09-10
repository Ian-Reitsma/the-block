use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;

#[derive(Deserialize)]
pub struct Metric {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unit: String,
}

pub fn generate_dashboard(metrics_path: &str, overrides_path: Option<&str>) -> Value {
    let root: Value = serde_json::from_str(&fs::read_to_string(metrics_path).unwrap()).unwrap();
    let metrics: Vec<Metric> = serde_json::from_value(root["metrics"].clone()).unwrap();
    let overrides = overrides_path
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());
    generate(&metrics, overrides)
}

fn generate(metrics: &[Metric], overrides: Option<Value>) -> Value {
    let panels: Vec<Value> = metrics
        .iter()
        .enumerate()
        .map(|(i, m)| {
            json!({
                "type": "timeseries",
                "id": i + 1,
                "title": m.name,
                "targets": [{"expr": m.name}],
                "description": format!("{} {}", m.description, m.unit).trim(),
            })
        })
        .collect();
    let mut dashboard = json!({ "panels": panels });
    if let Some(Value::Object(ov)) = overrides {
        if let Value::Object(ref mut base) = dashboard {
            for (k, v) in ov.into_iter() {
                base.insert(k, v);
            }
        }
    }
    dashboard
}
