use foundation_serialization::json::{self, Value};
use std::fs;

#[test]
fn dashboard_snapshot() {
    let generated = fs::read_to_string("grafana/dashboard.json").unwrap();
    let expected = fs::read_to_string("tests/snapshots/dashboard.json").unwrap();
    assert_eq!(generated, expected);
}

#[test]
fn dashboard_ack_latency_panel_includes_targets() {
    let generated = fs::read_to_string("grafana/dashboard.json").unwrap();
    let value: Value = json::from_str(&generated).expect("dashboard json");
    let panels = value
        .as_object()
        .and_then(|root| root.get("panels"))
        .and_then(Value::as_array)
        .expect("panels array");
    let panel = panels
        .iter()
        .find(|entry| {
            entry
                .as_object()
                .and_then(|map| map.get("title"))
                .and_then(Value::as_str)
                == Some("bridge_remediation_ack_latency_seconds (p50/p95)")
        })
        .expect("ack latency panel present");
    let targets = panel
        .as_object()
        .and_then(|map| map.get("targets"))
        .and_then(Value::as_array)
        .expect("targets array");
    let mut exprs = Vec::new();
    for target in targets {
        if let Some(expr) = target
            .as_object()
            .and_then(|map| map.get("expr"))
            .and_then(Value::as_str)
        {
            exprs.push(expr.to_string());
        }
    }
    assert!(exprs
        .iter()
        .any(|expr| expr == "bridge_remediation_ack_target_seconds{phase=\"retry\"}"));
    assert!(exprs
        .iter()
        .any(|expr| expr == "bridge_remediation_ack_target_seconds{phase=\"escalate\"}"));
}
