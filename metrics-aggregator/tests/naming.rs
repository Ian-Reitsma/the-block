use regex::Regex;
use serde_json::Value;
use std::fs;

#[test]
fn metric_names_follow_convention() {
    let data = fs::read_to_string("monitoring/metrics.json").unwrap();
    let v: Value = serde_json::from_str(&data).unwrap();
    let metrics = v["metrics"].as_array().unwrap();
    let re = Regex::new(r"^[a-z0-9_]+$").unwrap();
    for m in metrics {
        let name = m["name"].as_str().unwrap();
        assert!(re.is_match(name), "bad metric name {name}");
        if m["description"].as_str().unwrap_or("").contains("total") {
            assert!(name.ends_with("_total"), "missing _total suffix for {name}");
        }
    }
}
