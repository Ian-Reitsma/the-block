use crypto_suite::hashing::blake3;
use foundation_serialization::json::{self, Value};
use std::fs;

struct PanelExpectation<'a> {
    title: &'a str,
    expr: &'a str,
    legend: Option<&'a str>,
    description: Option<&'a str>,
}

fn load_dashboard(path: &str) -> Value {
    let generated = fs::read_to_string(path).expect("dashboard json");
    json::from_str(&generated).expect("dashboard value")
}

fn find_panel<'a>(panels: &'a [Value], title: &str) -> Option<&'a Value> {
    panels.iter().find(|entry| {
        entry
            .as_object()
            .and_then(|map| map.get("title"))
            .and_then(Value::as_str)
            == Some(title)
    })
}

fn assert_panel(path: &str, expectation: &PanelExpectation<'_>) {
    let value = load_dashboard(path);
    let panels = value
        .as_object()
        .and_then(|root| root.get("panels"))
        .and_then(Value::as_array)
        .expect("panels array");
    let panel = find_panel(panels, expectation.title)
        .unwrap_or_else(|| panic!("missing panel '{}' in {}", expectation.title, path));
    if let Some(expected_description) = expectation.description {
        let description = panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            description, expected_description,
            "mismatched description in {}",
            path
        );
    }
    let targets = panel
        .as_object()
        .and_then(|map| map.get("targets"))
        .and_then(Value::as_array)
        .expect("targets array");
    assert!(
        targets.iter().any(|target| {
            target
                .as_object()
                .and_then(|map| map.get("expr"))
                .and_then(Value::as_str)
                == Some(expectation.expr)
        }),
        "missing expression '{}' in {}",
        expectation.expr,
        path
    );

    if let Some(expected_legend) = expectation.legend {
        assert!(
            targets.iter().any(|target| {
                target
                    .as_object()
                    .and_then(|map| map.get("legendFormat"))
                    .and_then(Value::as_str)
                    == Some(expected_legend)
            }),
            "missing legend '{}' in {}",
            expected_legend,
            path
        );
    }
}

fn panel_targets<'a>(panel: &'a Value) -> &'a [Value] {
    panel
        .as_object()
        .and_then(|map| map.get("targets"))
        .and_then(Value::as_array)
        .expect("panel targets")
}

fn assert_target_expr_legend(panel: &Value, expr: &str, legend: Option<&str>) {
    let targets = panel_targets(panel);
    let target = targets
        .iter()
        .find(|entry| {
            entry
                .as_object()
                .and_then(|map| map.get("expr"))
                .and_then(Value::as_str)
                == Some(expr)
        })
        .unwrap_or_else(|| panic!("missing expr '{}'", expr));
    if let Some(expected) = legend {
        let actual = target
            .as_object()
            .and_then(|map| map.get("legendFormat"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(actual, expected, "legend mismatch for expr '{}'", expr);
    } else {
        assert!(target
            .as_object()
            .and_then(|map| map.get("legendFormat"))
            .is_none());
    }
}

#[test]
fn dashboard_snapshot() {
    if std::env::var("FIRST_PARTY_ONLY").ok().as_deref() == Some("1") {
        // In first-party-only mode we treat Grafana artifacts as legacy; just ensure the file exists and parses.
        let generated = fs::read_to_string("grafana/dashboard.json").unwrap();
        let _: Value = json::from_str(&generated).expect("dashboard json");
        return;
    }
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

#[test]
fn treasury_dashboard_snapshot_and_hash() {
    let content =
        fs::read_to_string("grafana_treasury_dashboard.json").expect("treasury dashboard json");
    let expected = fs::read_to_string("tests/snapshots/treasury_dashboard.json")
        .expect("treasury dashboard snapshot");
    assert_eq!(
        content, expected,
        "treasury dashboard drifted; run `make -C monitoring dashboard` to refresh"
    );
    let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    assert_eq!(
        hash.as_str(),
        "e9d9dc350aeedbe1167c6120b8f5600f7f079b3e2ffe9ab7542917de021a61a0",
        "treasury dashboard hash changed; refresh snapshot intentionally (current {})",
        hash
    );
}

#[test]
fn dashboards_include_bridge_counter_panels() {
    let dashboards = [
        "grafana/dashboard.json",
        "grafana/dev.json",
        "grafana/operator.json",
        "grafana/telemetry.json",
    ];
    let expectations = [
        PanelExpectation {
            title: "bridge_reward_claims_total (5m delta)",
            expr: "increase(bridge_reward_claims_total[5m])",
            legend: None,
            description: None,
        },
        PanelExpectation {
            title: "bridge_reward_approvals_consumed_total (5m delta)",
            expr: "increase(bridge_reward_approvals_consumed_total[5m])",
            legend: None,
            description: None,
        },
        PanelExpectation {
            title: "bridge_settlement_results_total (5m delta)",
            expr: "sum by (result, reason)(increase(bridge_settlement_results_total[5m]))",
            legend: Some("{{result}} · {{reason}}"),
            description: None,
        },
        PanelExpectation {
            title: "bridge_dispute_outcomes_total (5m delta)",
            expr: "sum by (kind, outcome)(increase(bridge_dispute_outcomes_total[5m]))",
            legend: Some("{{kind}} · {{outcome}}"),
            description: None,
        },
    ];

    for path in dashboards {
        for expectation in &expectations {
            assert_panel(path, expectation);
        }
    }
}

#[test]
fn dashboards_include_payout_last_seen_panels() {
    let dashboards = [
        "grafana/dashboard.json",
        "grafana/dev.json",
        "grafana/operator.json",
        "grafana/telemetry.json",
    ];
    let expectations = [
        PanelExpectation {
            title: "Read subsidy last seen (timestamp)",
            expr: "max by (role)(explorer_block_payout_read_last_seen_timestamp)",
            legend: Some("{{role}}"),
            description: None,
        },
        PanelExpectation {
            title: "Read subsidy staleness (seconds)",
            expr:
                "clamp_min(time() - max by (role)(explorer_block_payout_read_last_seen_timestamp), 0)",
            legend: Some("{{role}}"),
            description: None,
        },
        PanelExpectation {
            title: "Advertising payout last seen (timestamp)",
            expr: "max by (role)(explorer_block_payout_ad_last_seen_timestamp)",
            legend: Some("{{role}}"),
            description: None,
        },
        PanelExpectation {
            title: "Advertising payout staleness (seconds)",
            expr:
                "clamp_min(time() - max by (role)(explorer_block_payout_ad_last_seen_timestamp), 0)",
            legend: Some("{{role}}"),
            description: None,
        },
    ];
    for path in dashboards {
        for expectation in &expectations {
            assert_panel(path, expectation);
        }
    }
}

#[test]
fn dashboards_include_bridge_remediation_legends_and_tooltips() {
    let dashboards = [
        "grafana/dashboard.json",
        "grafana/dev.json",
        "grafana/operator.json",
        "grafana/telemetry.json",
    ];
    for path in dashboards {
        let value = load_dashboard(path);
        let panels = value
            .as_object()
            .and_then(|root| root.get("panels"))
            .and_then(Value::as_array)
            .expect("panels array");

        let action_panel = find_panel(panels, "bridge_remediation_action_total (5m delta)")
            .expect("action panel present");
        assert_target_expr_legend(
            action_panel,
            "sum by (action, playbook)(increase(bridge_remediation_action_total[5m]))",
            Some("{{action}} · {{playbook}}"),
        );
        let action_description = action_panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            action_description, "Bridge remediation actions grouped by outcome and playbook",
            "bridge_remediation_action_total tooltip drift in {}",
            path
        );

        let dispatch_panel = find_panel(panels, "bridge_remediation_dispatch_total (5m delta)")
            .expect("dispatch panel present");
        assert_target_expr_legend(
            dispatch_panel,
            "sum by (action, playbook, target, status)(increase(bridge_remediation_dispatch_total[5m]))",
            Some("{{action}} · {{target}} · {{status}}"),
        );
        let dispatch_description = dispatch_panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            dispatch_description,
            "Bridge remediation dispatch attempts grouped by target surface and status",
            "bridge_remediation_dispatch_total tooltip drift in {}",
            path
        );

        let ack_panel = find_panel(panels, "bridge_remediation_dispatch_ack_total (5m delta)")
            .expect("dispatch ack panel present");
        assert_target_expr_legend(
            ack_panel,
            "sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))",
            Some("{{action}} · {{target}} · {{state}}"),
        );
        let ack_description = ack_panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            ack_description,
            "Bridge remediation dispatch acknowledgements grouped by target and state",
            "bridge_remediation_dispatch_ack_total tooltip drift in {}",
            path
        );

        let latency_panel = find_panel(panels, "bridge_remediation_ack_latency_seconds (p50/p95)")
            .expect("ack latency panel present");
        assert_target_expr_legend(
            latency_panel,
            "histogram_quantile(0.50, sum by (le, playbook, state)(rate(bridge_remediation_ack_latency_seconds_bucket[5m])))",
            Some("{playbook} · {state} · p50"),
        );
        assert_target_expr_legend(
            latency_panel,
            "histogram_quantile(0.95, sum by (le, playbook, state)(rate(bridge_remediation_ack_latency_seconds_bucket[5m])))",
            Some("{playbook} · {state} · p95"),
        );
        assert_target_expr_legend(
            latency_panel,
            "bridge_remediation_ack_target_seconds{phase=\"retry\"}",
            Some("{playbook} · retry target"),
        );
        assert_target_expr_legend(
            latency_panel,
            "bridge_remediation_ack_target_seconds{phase=\"escalate\"}",
            Some("{playbook} · escalate target"),
        );
        let latency_description = latency_panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            latency_description,
            "Bridge remediation acknowledgement latency grouped by playbook and state",
            "bridge_remediation_ack_latency_seconds tooltip drift in {}",
            path
        );

        let spool_panel =
            find_panel(panels, "bridge_remediation_spool_artifacts").expect("spool panel present");
        assert_target_expr_legend(spool_panel, "bridge_remediation_spool_artifacts", None);
        let spool_description = spool_panel
            .as_object()
            .and_then(|map| map.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            spool_description,
            "Outstanding bridge remediation spool artifacts awaiting acknowledgement",
            "bridge_remediation_spool_artifacts tooltip drift in {}",
            path
        );
    }
}
