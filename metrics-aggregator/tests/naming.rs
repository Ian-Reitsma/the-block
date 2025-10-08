use foundation_serialization::json::{self, Value};
use httpd::filters::Regex;
use std::fs;
use std::path::PathBuf;

#[test]
fn metric_names_follow_convention() {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "monitoring",
        "metrics.json",
    ]
    .iter()
    .collect();
    let data = fs::read_to_string(path).unwrap();
    let v: Value = json::from_str(&data).unwrap();
    let metrics = v
        .as_object()
        .and_then(|map| map.get("metrics"))
        .and_then(|value| value.as_array())
        .unwrap();
    let re = Regex::new(r"^[a-z0-9_]+$").unwrap();
    for metric in metrics {
        let metric_obj = metric.as_object().unwrap();
        let name = metric_obj
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap();
        assert!(re.is_match(name), "bad metric name {name}");
        let description = metric_obj
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if description.contains("total") {
            assert!(name.ends_with("_total"), "missing _total suffix for {name}");
        }
    }
}
