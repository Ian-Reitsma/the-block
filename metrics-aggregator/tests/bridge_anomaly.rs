use foundation_serialization::json::{self, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{
    install_bridge_http_client_override, reset_bridge_remediation_ack_metrics,
    reset_bridge_remediation_dispatch_log, router, AppState, BridgeHttpClientOverride,
    BridgeHttpClientOverrideGuard, BridgeHttpOverrideResponse,
};
use std::env;
use std::fs;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sys::tempfile::{self, NamedTempFile};

struct ApiAnomalyLabel {
    key: String,
    value: String,
}

struct ApiAnomalyEvent {
    metric: String,
    peer_id: String,
    _delta: f64,
    labels: Vec<ApiAnomalyLabel>,
}

fn parse_anomaly_events(bytes: &[u8]) -> Vec<ApiAnomalyEvent> {
    let value: Value = json::from_slice(bytes).expect("anomaly response json");
    let array = value.as_array().expect("response array");
    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().expect("event object");
            let labels = object
                .get("labels")
                .and_then(Value::as_array)
                .expect("labels array")
                .iter()
                .map(|label| {
                    let label_obj = label.as_object().expect("label object");
                    ApiAnomalyLabel {
                        key: label_obj
                            .get("key")
                            .and_then(Value::as_str)
                            .expect("label key")
                            .to_string(),
                        value: label_obj
                            .get("value")
                            .and_then(Value::as_str)
                            .expect("label value")
                            .to_string(),
                    }
                })
                .collect();
            ApiAnomalyEvent {
                metric: object
                    .get("metric")
                    .and_then(Value::as_str)
                    .expect("metric")
                    .to_string(),
                peer_id: object
                    .get("peer_id")
                    .and_then(Value::as_str)
                    .expect("peer_id")
                    .to_string(),
                _delta: object.get("delta").and_then(Value::as_f64).expect("delta"),
                labels,
            }
        })
        .collect()
}

struct ApiRemediationAction {
    action: String,
    peer_id: String,
    metric: String,
    occurrences: u64,
    labels: Vec<ApiAnomalyLabel>,
    playbook: String,
    annotation: Option<String>,
    runbook_path: Option<String>,
    dispatch_endpoint: Option<String>,
    response_sequence: Vec<String>,
    dashboard_panels: Vec<String>,
    acknowledged_at: Option<u64>,
    closed_out_at: Option<u64>,
    acknowledgement_notes: Option<String>,
    dispatch_attempts: u64,
    auto_retry_count: u64,
    follow_up_notes: Option<String>,
    last_dispatch_at: Option<u64>,
    pending_since: Option<u64>,
    spool_artifacts: Vec<String>,
}

fn parse_remediation_actions(bytes: &[u8]) -> Vec<ApiRemediationAction> {
    let value: Value = json::from_slice(bytes).expect("remediation response json");
    let array = value.as_array().expect("response array");
    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().expect("action object");
            let labels = object
                .get("labels")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .map(|label| {
                            let label_obj = label.as_object().expect("label object");
                            ApiAnomalyLabel {
                                key: label_obj
                                    .get("key")
                                    .and_then(Value::as_str)
                                    .expect("label key")
                                    .to_string(),
                                value: label_obj
                                    .get("value")
                                    .and_then(Value::as_str)
                                    .expect("label value")
                                    .to_string(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_else(Vec::new);
            let response_sequence = object
                .get("response_sequence")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            let dashboard_panels = object
                .get("dashboard_panels")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            let spool_artifacts = object
                .get("spool_artifacts")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            ApiRemediationAction {
                action: object
                    .get("action")
                    .and_then(Value::as_str)
                    .expect("action field")
                    .to_string(),
                playbook: object
                    .get("playbook")
                    .and_then(Value::as_str)
                    .unwrap_or("none")
                    .to_string(),
                peer_id: object
                    .get("peer_id")
                    .and_then(Value::as_str)
                    .expect("peer_id field")
                    .to_string(),
                metric: object
                    .get("metric")
                    .and_then(Value::as_str)
                    .expect("metric field")
                    .to_string(),
                occurrences: object
                    .get("occurrences")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                labels,
                annotation: object
                    .get("annotation")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                runbook_path: object
                    .get("runbook_path")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                dispatch_endpoint: object
                    .get("dispatch_endpoint")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                response_sequence,
                dashboard_panels,
                acknowledged_at: match object.get("acknowledged_at") {
                    Some(Value::Null) | None => None,
                    Some(value) => value.as_u64(),
                },
                closed_out_at: match object.get("closed_out_at") {
                    Some(Value::Null) | None => None,
                    Some(value) => value.as_u64(),
                },
                acknowledgement_notes: object
                    .get("acknowledgement_notes")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                dispatch_attempts: object
                    .get("dispatch_attempts")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                auto_retry_count: object
                    .get("auto_retry_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                follow_up_notes: object
                    .get("follow_up_notes")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                last_dispatch_at: match object.get("last_dispatch_at") {
                    Some(Value::Null) | None => None,
                    Some(value) => value.as_u64(),
                },
                pending_since: match object.get("pending_since") {
                    Some(Value::Null) | None => None,
                    Some(value) => value.as_u64(),
                },
                spool_artifacts,
            }
        })
        .collect()
}

struct ApiDispatchRecord {
    target: String,
    status: String,
    annotation: String,
    response_sequence: Vec<String>,
    dashboard_panels: Vec<String>,
    acknowledgement_state: Option<String>,
    acknowledgement_acknowledged: Option<bool>,
    acknowledgement_closed: Option<bool>,
    acknowledgement_notes: Option<String>,
    acknowledgement_timestamp: Option<u64>,
}

fn parse_dispatch_records(bytes: &[u8]) -> Vec<ApiDispatchRecord> {
    let value: Value = json::from_slice(bytes).expect("dispatch response json");
    let array = value.as_array().expect("response array");
    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().expect("dispatch object");
            let response_sequence = object
                .get("response_sequence")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            let dashboard_panels = object
                .get("dashboard_panels")
                .and_then(Value::as_array)
                .map(|array| {
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(Vec::new);
            let acknowledgement = object.get("acknowledgement").and_then(Value::as_object);
            let acknowledgement_state = acknowledgement
                .and_then(|map| map.get("state"))
                .and_then(Value::as_str)
                .map(|value| value.to_string());
            let acknowledgement_acknowledged = acknowledgement
                .and_then(|map| map.get("acknowledged"))
                .and_then(Value::as_bool);
            let acknowledgement_closed = acknowledgement
                .and_then(|map| map.get("closed"))
                .and_then(Value::as_bool);
            let acknowledgement_notes = acknowledgement
                .and_then(|map| map.get("notes"))
                .and_then(Value::as_str)
                .map(|value| value.to_string());
            let acknowledgement_timestamp = acknowledgement
                .and_then(|map| map.get("timestamp"))
                .and_then(Value::as_u64);
            ApiDispatchRecord {
                target: object
                    .get("target")
                    .and_then(Value::as_str)
                    .expect("target field")
                    .to_string(),
                status: object
                    .get("status")
                    .and_then(Value::as_str)
                    .expect("status field")
                    .to_string(),
                annotation: object
                    .get("annotation")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                response_sequence,
                dashboard_panels,
                acknowledgement_state,
                acknowledgement_acknowledged,
                acknowledgement_closed,
                acknowledgement_notes,
                acknowledgement_timestamp,
            }
        })
        .collect()
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var(key).ok();
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.previous.take() {
            env::set_var(self.key, prev);
        } else {
            env::remove_var(self.key);
        }
    }
}

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[derive(Clone)]
enum HookResponse {
    Json(Value),
    Text(String),
}

struct OverrideHttpClient {
    captured: Arc<Mutex<Vec<Value>>>,
    response: HookResponse,
}

impl BridgeHttpClientOverride for OverrideHttpClient {
    fn send(&self, _url: &str, payload: &Value) -> Result<BridgeHttpOverrideResponse, String> {
        self.captured
            .lock()
            .expect("capture lock")
            .push(payload.clone());
        match &self.response {
            HookResponse::Json(value) => json::to_vec(value)
                .map(|body| BridgeHttpOverrideResponse {
                    status: StatusCode::OK,
                    body,
                })
                .map_err(|err| err.to_string()),
            HookResponse::Text(body) => Ok(BridgeHttpOverrideResponse {
                status: StatusCode::OK,
                body: body.as_bytes().to_vec(),
            }),
        }
    }
}

fn install_http_override(
    response: HookResponse,
) -> (BridgeHttpClientOverrideGuard, Arc<Mutex<Vec<Value>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let client = Arc::new(OverrideHttpClient {
        captured: Arc::clone(&captured),
        response,
    });
    let guard = install_bridge_http_client_override(client);
    (guard, captured)
}

async fn wait_for_requests(captured: &Arc<Mutex<Vec<Value>>>, expected: usize) -> usize {
    for _ in 0..50 {
        let len = captured.lock().unwrap().len();
        if len >= expected {
            return len;
        }
        runtime::sleep(Duration::from_millis(20)).await;
    }
    captured.lock().unwrap().len()
}

fn build_ingest_payload(value: u64) -> Value {
    json::value_from_str(&format!(
        r#"[
            {{
                "peer_id": "bridge-node",
                "metrics": {{
                    "bridge_reward_claims_total": [
                        {{"value": {value}}}
                    ]
                }}
            }}
        ]"#
    ))
    .expect("valid json")
}

fn build_labeled_payload(value: u64, asset: &str) -> Value {
    json::value_from_str(&format!(
        r#"[
            {{
                "peer_id": "bridge-node",
                "metrics": {{
                    "bridge_settlement_results_total": [
                        {{
                            "labels": {{
                                "asset": "{asset}",
                                "result": "success",
                                "reason": "ok"
                            }},
                            "value": {value}
                        }}
                    ]
                }}
            }}
        ]"#
    ))
    .expect("valid json")
}

fn build_liquidity_payload(metric: &str, value: u64, asset: &str) -> Value {
    json::value_from_str(&format!(
        r#"[
            {{
                "peer_id": "bridge-node",
                "metrics": {{
                    "{metric}": [
                        {{
                            "labels": {{
                                "asset": "{asset}"
                            }},
                            "value": {value}
                        }}
                    ]
                }}
            }}
        ]"#
    ))
    .expect("valid json")
}

fn scrape_metric_value(body: &str, metric: &str, labels: &str) -> Option<f64> {
    if labels.is_empty() {
        let direct_prefix = format!("{metric} ");
        if let Some(value) = body.lines().find_map(|line| {
            line.strip_prefix(&direct_prefix)
                .and_then(|rest| rest.trim().parse::<f64>().ok())
        }) {
            return Some(value);
        }
        let bracket_prefix = format!("{metric}{{}}");
        if let Some(value) = body.lines().find_map(|line| {
            line.strip_prefix(&bracket_prefix)
                .and_then(|rest| rest.trim().parse::<f64>().ok())
        }) {
            return Some(value);
        }
    }
    let needle = format!("{metric}{{{labels}}}");
    body.lines()
        .find_map(|line| line.strip_prefix(&needle)?.trim().parse::<f64>().ok())
}

#[test]
fn bridge_anomaly_detector_flags_spikes() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in 1..=7u64 {
            let payload = build_ingest_payload(value);
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(1100)).await;
        }

        let spike_payload = build_ingest_payload(120);
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let anomalies_resp = app
            .handle(app.request_builder().path("/anomalies/bridge").build())
            .await
            .unwrap();
        assert_eq!(anomalies_resp.status(), StatusCode::OK);
        let events = parse_anomaly_events(anomalies_resp.body());
        assert!(!events.is_empty(), "expected at least one anomaly event");
        assert!(events
            .iter()
            .any(|event| event.metric == "bridge_reward_claims_total"
                && event.peer_id == "bridge-node"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(
            metrics_body.contains("bridge_anomaly_total"),
            "expected metrics payload to include bridge_anomaly_total"
        );
        assert!(
            metrics_body.contains(
                "bridge_metric_delta{metric=\"bridge_reward_claims_total\",peer=\"bridge-node\",labels=\"\"}"
            ),
            "expected unlabeled delta gauge for bridge_reward_claims_total"
        );
        assert!(
            metrics_body.contains(
                "bridge_metric_rate_per_second{metric=\"bridge_reward_claims_total\",peer=\"bridge-node\",labels=\"\"}"
            ),
            "expected unlabeled rate gauge for bridge_reward_claims_total"
        );
    });
}

#[test]
fn bridge_anomaly_detector_respects_cooldown_and_labels() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(50)).await;
        }

        let spike_payload = build_labeled_payload(80, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let anomalies_resp = app
            .handle(app.request_builder().path("/anomalies/bridge").build())
            .await
            .unwrap();
        assert_eq!(anomalies_resp.status(), StatusCode::OK);
        let events = parse_anomaly_events(anomalies_resp.body());
        assert_eq!(events.len(), 1);
        let labels: Vec<_> = events[0]
            .labels
            .iter()
            .map(|label| (label.key.clone(), label.value.clone()))
            .collect();
        assert!(labels.contains(&("asset".into(), "eth".into())));
        assert!(labels.contains(&("result".into(), "success".into())));
        assert!(labels.contains(&("reason".into(), "ok".into())));

        let rapid_spike = build_labeled_payload(160, "eth");
        let rapid_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&rapid_spike)
            .unwrap()
            .build();
        let rapid_resp = app.handle(rapid_req).await.unwrap();
        assert_eq!(rapid_resp.status(), StatusCode::OK);

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(
            metrics_body.contains(
                "bridge_metric_delta{metric=\"bridge_settlement_results_total\",peer=\"bridge-node\",labels=\"asset=eth,reason=ok,result=success\"}"
            ),
            "expected labeled delta gauge for bridge_settlement_results_total"
        );
        assert!(
            metrics_body.contains(
                "bridge_metric_rate_per_second{metric=\"bridge_settlement_results_total\",peer=\"bridge-node\",labels=\"asset=eth,reason=ok,result=success\"}"
            ),
            "expected labeled rate gauge for bridge_settlement_results_total"
        );

        let post_resp = app
            .handle(app.request_builder().path("/anomalies/bridge").build())
            .await
            .unwrap();
        let post_events = parse_anomaly_events(post_resp.body());
        assert_eq!(
            post_events.len(),
            1,
            "cooldown should suppress duplicate anomalies"
        );

        let drop_payload = build_labeled_payload(10, "eth");
        let drop_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&drop_payload)
            .unwrap()
            .build();
        let drop_resp = app.handle(drop_req).await.unwrap();
        assert_eq!(drop_resp.status(), StatusCode::OK);

        let final_resp = app
            .handle(app.request_builder().path("/anomalies/bridge").build())
            .await
            .unwrap();
        let final_events = parse_anomaly_events(final_resp.body());
        assert_eq!(final_events.len(), 1);
    });
}

#[test]
fn bridge_remediation_exposes_actions() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(50)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        assert!(!actions.is_empty(), "expected remediation action");
        let action = actions.last().unwrap();
        assert_eq!(action.action, "escalate");
        assert_eq!(action.playbook, "governance-escalation");
        assert_eq!(action.peer_id, "bridge-node");
        assert_eq!(action.metric, "bridge_settlement_results_total");
        assert!(action.occurrences >= 1);
        assert!(action
            .labels
            .iter()
            .any(|label| label.key == "asset" && label.value == "eth"));
        let annotation = action.annotation.as_ref().expect("annotation field");
        assert!(annotation.contains("governance escalation"));
        assert!(annotation.contains("bridge-node"));
        assert_eq!(
            action.runbook_path.as_deref(),
            Some("docs/operators/incident_playbook.md#bridge-liquidity-remediation")
        );
        assert_eq!(
            action.dispatch_endpoint.as_deref(),
            Some("/remediation/bridge/dispatches")
        );
        assert!(
            !action.response_sequence.is_empty(),
            "expected response sequence"
        );
        assert!(action
            .response_sequence
            .iter()
            .any(|step| step.contains("/remediation/bridge/dispatches")));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_total (5m delta)"));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_ack_total (5m delta)"));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_ack_latency_seconds (p50/p95)"));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_ack_total (5m delta)"));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_ack_latency_seconds (p50/p95)"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            "bridge_remediation_action_total{action=\"escalate\",playbook=\"governance-escalation\"}"
        ));
    });
}

#[test]
fn bridge_remediation_emits_throttle_playbook() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 11, 12, 13, 14, 15, 16] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(50)).await;
        }

        let spike_payload = build_labeled_payload(25, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        assert!(!actions.is_empty(), "expected remediation action");
        let action = actions.last().unwrap();
        assert_eq!(action.action, "throttle");
        assert_eq!(action.playbook, "incentive-throttle");
        let annotation = action.annotation.as_ref().expect("annotation field");
        assert!(annotation.contains("incentive throttle"));
        assert_eq!(
            action.runbook_path.as_deref(),
            Some("docs/operators/incident_playbook.md#bridge-liquidity-remediation")
        );
        assert!(action
            .response_sequence
            .iter()
            .any(|step| step.contains("incentive throttle")));
        assert!(action
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_total (5m delta)"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            "bridge_remediation_action_total{action=\"throttle\",playbook=\"incentive-throttle\"}"
        ));
    });
}

#[test]
fn bridge_remediation_dispatches_to_spool_hooks() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let spool = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(
            "TB_REMEDIATION_ESCALATE_DIRS",
            spool.path().to_str().expect("spool path str"),
        );
        let _url_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(150)).await;

        let mut entries: Vec<_> = fs::read_dir(spool.path())
            .expect("read spool directory")
            .filter_map(|entry| entry.ok())
            .collect();
        entries.sort_by_key(|entry| entry.path());
        assert!(
            !entries.is_empty(),
            "expected remediation hook to persist a spool entry",
        );
        let path = entries.last().unwrap().path();
        let bytes = fs::read(&path).expect("read spool payload");
        let payload: Value = json::from_slice(&bytes).expect("decode spool payload");
        let object = payload.as_object().expect("payload object");
        assert_eq!(
            object
                .get("action")
                .and_then(Value::as_str)
                .expect("action field"),
            "escalate",
        );
        assert_eq!(
            object
                .get("playbook")
                .and_then(Value::as_str)
                .expect("playbook field"),
            "governance-escalation",
        );
        assert_eq!(
            object
                .get("metric")
                .and_then(Value::as_str)
                .expect("metric field"),
            "bridge_settlement_results_total",
        );
        assert_eq!(
            object
                .get("peer_id")
                .and_then(Value::as_str)
                .expect("peer field"),
            "bridge-node",
        );
        assert!(
            object
                .get("dispatched_at")
                .and_then(Value::as_u64)
                .is_some(),
            "dispatch timestamp missing",
        );
        let annotation = object
            .get("annotation")
            .and_then(Value::as_str)
            .expect("annotation field");
        assert!(annotation.contains("bridge-node"));
        assert!(annotation.contains("governance escalation"));
        assert_eq!(
            object
                .get("runbook_path")
                .and_then(Value::as_str)
                .expect("runbook path"),
            "docs/operators/incident_playbook.md#bridge-liquidity-remediation",
        );
        assert_eq!(
            object
                .get("dispatch_endpoint")
                .and_then(Value::as_str)
                .expect("dispatch endpoint"),
            "/remediation/bridge/dispatches",
        );
        let steps = object
            .get("response_sequence")
            .and_then(Value::as_array)
            .expect("response sequence");
        assert!(!steps.is_empty(), "expected response steps");
        assert!(steps.iter().any(|entry| {
            entry
                .as_str()
                .map(|text| text.contains("/remediation/bridge/dispatches"))
                .unwrap_or(false)
        }));
        let panels = object
            .get("dashboard_panels")
            .and_then(Value::as_array)
            .expect("dashboard panels");
        assert!(panels.iter().any(|panel| {
            panel
                .as_str()
                .map(|value| value == "bridge_remediation_dispatch_total (5m delta)")
                .unwrap_or(false)
        }));
        assert!(panels.iter().any(|panel| {
            panel
                .as_str()
                .map(|value| value == "bridge_remediation_dispatch_ack_total (5m delta)")
                .unwrap_or(false)
        }));
        assert!(panels.iter().any(|panel| {
            panel
                .as_str()
                .map(|value| value == "bridge_remediation_ack_latency_seconds (p50/p95)")
                .unwrap_or(false)
        }));

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert_eq!(record.target, "spool");
        assert_eq!(record.status, "success");
        assert!(record.annotation.contains("bridge-node"));
        assert!(record
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_total (5m delta)"));
        assert!(record
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_dispatch_ack_total (5m delta)"));
        assert!(record
            .dashboard_panels
            .iter()
            .any(|panel| panel == "bridge_remediation_ack_latency_seconds (p50/p95)"));
        assert!(record
            .response_sequence
            .iter()
            .any(|step| step.contains("/remediation/bridge/dispatches")));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_total{action="escalate",playbook="governance-escalation",target="spool",status="success"}"#
        ));
    });
}

#[test]
fn bridge_remediation_records_http_acknowledgements() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":true,"notes":"pager"}"#)
                .expect("ack response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(200)).await;
        let observed = wait_for_requests(&captured, 1).await;

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert!(
            observed >= 1,
            "expected http acknowledgement dispatch, observed {} events; target={}, status={}",
            observed,
            record.target,
            record.status
        );
        assert_eq!(record.target, "http");
        assert_eq!(record.status, "success");
        assert_eq!(
            record.acknowledgement_state.as_deref(),
            Some("acknowledged")
        );
        assert_eq!(record.acknowledgement_acknowledged, Some(true));
        assert_eq!(record.acknowledgement_closed, Some(false));
        assert!(record.acknowledgement_timestamp.unwrap_or(0) > 0);
        assert_eq!(record.acknowledgement_notes.as_deref(), Some("pager"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_ack_total{action="escalate",playbook="governance-escalation",target="http",state="acknowledged"}"#
        ));
        assert!(metrics_body.contains(
            r#"bridge_remediation_ack_latency_seconds_bucket{playbook="governance-escalation",state="acknowledged""#
        ));

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        assert!(!actions.is_empty(), "expected remediation action");
        let action = actions.last().unwrap();
        assert!(action.acknowledged_at.is_some());
        assert!(action.closed_out_at.is_none());
        assert_eq!(action.acknowledgement_notes.as_deref(), Some("pager"));
    });
}

#[test]
fn bridge_remediation_parses_text_acknowledgements() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let response = HookResponse::Text("acknowledged: pager".to_string());
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(200)).await;
        let observed = wait_for_requests(&captured, 1).await;

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert!(
            observed >= 1,
            "expected http acknowledgement dispatch, observed {} events; target={}, status={}",
            observed,
            record.target,
            record.status
        );
        assert_eq!(record.target, "http");
        assert_eq!(record.status, "success");
        assert_eq!(
            record.acknowledgement_state.as_deref(),
            Some("acknowledged")
        );
        assert_eq!(record.acknowledgement_acknowledged, Some(true));
        assert_eq!(record.acknowledgement_closed, Some(false));
        assert!(record.acknowledgement_timestamp.unwrap_or(0) > 0);
        assert_eq!(record.acknowledgement_notes.as_deref(), Some("pager"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_ack_total{action="escalate",playbook="governance-escalation",target="http",state="acknowledged"}"#
        ));
        assert!(metrics_body.contains(
            r#"bridge_remediation_ack_latency_seconds_bucket{playbook="governance-escalation",state="acknowledged""#
        ));
    });
}

#[test]
fn bridge_remediation_retries_pending_acknowledgements() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":false,"notes":"pending"}"#)
                .expect("pending response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _hook_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");
        let _retry_guard = EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS", "1");
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ACK_ESCALATE_SECS", "5");
        let _max_guard = EnvGuard::set("TB_REMEDIATION_ACK_MAX_RETRIES", "2");
        let _cleanup_guard = EnvGuard::set("AGGREGATOR_CLEANUP_INTERVAL_SECS", "1");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        state.spawn_cleanup();
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        wait_for_requests(&captured, 1).await;
        runtime::sleep(Duration::from_millis(1500)).await;
        let observed = wait_for_requests(&captured, 2).await;
        assert!(
            observed >= 2,
            "expected retry dispatch, observed {observed}"
        );

        let mut attempts = 0u64;
        let mut retries = 0u64;
        let mut notes = String::new();
        for _ in 0..50 {
            let remediation_resp = app
                .handle(app.request_builder().path("/remediation/bridge").build())
                .await
                .unwrap();
            assert_eq!(remediation_resp.status(), StatusCode::OK);
            let actions = parse_remediation_actions(remediation_resp.body());
            assert!(!actions.is_empty(), "expected remediation action");
            let action = actions.last().unwrap();
            attempts = action.dispatch_attempts;
            retries = action.auto_retry_count;
            notes = action.follow_up_notes.as_deref().unwrap_or("").to_string();
            if attempts >= 2 && retries >= 1 {
                break;
            }
            runtime::sleep(Duration::from_millis(40)).await;
        }
        assert!(
            attempts >= 2,
            "expected dispatch attempts >= 2, found {}",
            attempts
        );
        assert!(
            retries >= 1,
            "expected auto retry count >= 1, found {}",
            retries
        );
        assert!(notes.contains("retry"));
    });
}

#[test]
fn bridge_remediation_escalates_pending_acknowledgements() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let spool = tempfile::tempdir().unwrap();
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":false,"notes":"waiting"}"#)
                .expect("pending response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _throttle_guard = EnvGuard::set("TB_REMEDIATION_THROTTLE_URLS", "http://override/hook");
        let _escalate_dir_guard = EnvGuard::set(
            "TB_REMEDIATION_ESCALATE_DIRS",
            spool.path().to_str().expect("spool path"),
        );
        let _retry_guard = EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS", "1");
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ACK_ESCALATE_SECS", "3");
        let _max_guard = EnvGuard::set("TB_REMEDIATION_ACK_MAX_RETRIES", "1");
        let _cleanup_guard = EnvGuard::set("AGGREGATOR_CLEANUP_INTERVAL_SECS", "1");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        state.spawn_cleanup();
        let app = router(state);

        for value in [10u64, 11, 12, 13, 14, 15, 16] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(25, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        wait_for_requests(&captured, 1).await;
        runtime::sleep(Duration::from_secs(4)).await;
        let observed = wait_for_requests(&captured, 2).await;
        assert!(observed >= 2, "expected retry dispatch for throttle action");

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        assert!(
            actions.len() >= 2,
            "expected throttle and escalation actions"
        );
        let throttle = actions
            .iter()
            .rev()
            .find(|action| action.action == "throttle")
            .expect("throttle action present");
        assert!(throttle.dispatch_attempts >= 2);
        assert!(throttle.auto_retry_count >= 1);
        assert!(throttle.last_dispatch_at.is_some());
        assert!(throttle.pending_since.is_some());
        assert!(throttle
            .follow_up_notes
            .as_deref()
            .unwrap_or("")
            .contains("retry"));
        let escalate_action = actions
            .iter()
            .find(|action| action.action == "escalate")
            .expect("escalation action present");
        assert!(escalate_action.last_dispatch_at.is_some());
        assert!(escalate_action.pending_since.is_some());
        assert!(escalate_action
            .follow_up_notes
            .as_deref()
            .unwrap_or("")
            .contains("escalation"));
    });
}

#[test]
fn bridge_remediation_ack_policy_respects_playbook_overrides() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":false,"notes":"pending"}"#)
                .expect("pending response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");
        let _retry_guard = EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS", "1");
        let _max_guard = EnvGuard::set("TB_REMEDIATION_ACK_MAX_RETRIES", "3");
        let _default_escalate_guard = EnvGuard::set("TB_REMEDIATION_ACK_ESCALATE_SECS", "4");
        let _override_retry_guard =
            EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS_GOVERNANCE_ESCALATION", "15");
        let _override_escalate_guard = EnvGuard::set(
            "TB_REMEDIATION_ACK_ESCALATE_SECS_GOVERNANCE_ESCALATION",
            "30",
        );
        let _cleanup_guard = EnvGuard::set("AGGREGATOR_CLEANUP_INTERVAL_SECS", "1");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        state.spawn_cleanup();
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let initial = wait_for_requests(&captured, 1).await;
        assert!(initial >= 1, "expected escalation dispatch");

        runtime::sleep(Duration::from_secs(6)).await;
        let final_count = captured.lock().unwrap().len();
        assert_eq!(final_count, initial, "override delayed retries");

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        let escalation = actions
            .iter()
            .find(|action| action.action == "escalate")
            .expect("escalation action present");
        assert_eq!(escalation.auto_retry_count, 0);
        assert_eq!(escalation.dispatch_attempts, 1);
    });
}

#[test]
fn bridge_remediation_records_http_closure_acknowledgements() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":true,"closed":true,"notes":"resolved"}"#)
                .expect("closure response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [11u64, 13, 14, 16, 18, 21, 24] {
            let payload = build_labeled_payload(value, "btc");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(220, "btc");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(200)).await;
        let observed = wait_for_requests(&captured, 1).await;

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert!(
            observed >= 1,
            "expected http acknowledgement dispatch, observed {} events; target={}, status={}",
            observed,
            record.target,
            record.status
        );
        assert_eq!(record.target, "http");
        assert_eq!(record.status, "success");
        assert_eq!(record.acknowledgement_state.as_deref(), Some("closed"));
        assert_eq!(record.acknowledgement_acknowledged, Some(true));
        assert_eq!(record.acknowledgement_closed, Some(true));
        assert!(record.acknowledgement_timestamp.unwrap_or(0) > 0);
        assert_eq!(record.acknowledgement_notes.as_deref(), Some("resolved"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_ack_total{action="escalate",playbook="governance-escalation",target="http",state="closed"}"#
        ));
        assert!(metrics_body.contains(
            r#"bridge_remediation_ack_latency_seconds_bucket{playbook="governance-escalation",state="closed""#
        ));

        let remediation_resp = app
            .handle(app.request_builder().path("/remediation/bridge").build())
            .await
            .unwrap();
        assert_eq!(remediation_resp.status(), StatusCode::OK);
        let actions = parse_remediation_actions(remediation_resp.body());
        assert!(!actions.is_empty(), "expected remediation action");
        let action = actions.last().unwrap();
        assert!(action.acknowledged_at.is_some());
        assert!(action.closed_out_at.is_some());
        assert_eq!(action.acknowledgement_notes.as_deref(), Some("resolved"));
    });
}

#[test]
fn bridge_remediation_records_spool_failures() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let dir = tempfile::tempdir().unwrap();
        let spool_file = NamedTempFile::new().unwrap();
        let spool_path = spool_file.path().to_path_buf();
        let _guard = EnvGuard::set(
            "TB_REMEDIATION_ESCALATE_DIRS",
            spool_path.to_str().expect("spool file path"),
        );

        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(150)).await;

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert_eq!(record.target, "spool");
        assert_eq!(record.status, "persist_failed");
        assert!(record.annotation.contains("bridge-node"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_total{action="escalate",playbook="governance-escalation",target="spool",status="persist_failed"}"#
        ));
    });
}

#[test]
fn bridge_remediation_records_skipped_dispatch_when_unconfigured() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        let _url_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "");
        let _dir_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_DIRS", "");
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [10u64, 12, 13, 15, 17, 20, 21] {
            let payload = build_labeled_payload(value, "eth");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(20)).await;
        }

        let spike_payload = build_labeled_payload(160, "eth");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        runtime::sleep(Duration::from_millis(150)).await;

        let dispatch_resp = app
            .handle(
                app.request_builder()
                    .path("/remediation/bridge/dispatches")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(dispatch_resp.status(), StatusCode::OK);
        let dispatch_records = parse_dispatch_records(dispatch_resp.body());
        assert!(!dispatch_records.is_empty(), "expected dispatch record");
        let record = dispatch_records.last().unwrap();
        assert_eq!(record.target, "none");
        assert_eq!(record.status, "skipped");
        assert!(record.annotation.contains("bridge-node"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            r#"bridge_remediation_dispatch_total{action="escalate",playbook="governance-escalation",target="none",status="skipped"}"#
        ));
    });
}

#[test]
fn bridge_anomaly_flags_liquidity_spikes() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        for value in [20u64, 22, 23, 25, 27, 29, 31] {
            let payload = build_liquidity_payload("bridge_liquidity_locked_total", value, "btc");
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            runtime::sleep(Duration::from_millis(50)).await;
        }

        let spike_payload = build_liquidity_payload("bridge_liquidity_locked_total", 120, "btc");
        let spike_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&spike_payload)
            .unwrap()
            .build();
        let spike_resp = app.handle(spike_req).await.unwrap();
        assert_eq!(spike_resp.status(), StatusCode::OK);

        let anomalies_resp = app
            .handle(app.request_builder().path("/anomalies/bridge").build())
            .await
            .unwrap();
        let events = parse_anomaly_events(anomalies_resp.body());
        let locked_event = events
            .iter()
            .find(|event| event.metric == "bridge_liquidity_locked_total")
            .expect("expected liquidity anomaly");
        assert_eq!(locked_event.peer_id, "bridge-node");
        assert!(locked_event
            .labels
            .iter()
            .any(|label| label.key == "asset" && label.value == "btc"));

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
        assert!(metrics_body.contains(
            "bridge_metric_delta{metric=\"bridge_liquidity_locked_total\",peer=\"bridge-node\",labels=\"asset=btc\"}"
        ));
        assert!(metrics_body.contains(
            "bridge_metric_rate_per_second{metric=\"bridge_liquidity_locked_total\",peer=\"bridge-node\",labels=\"asset=btc\"}"
        ));
    });
}

#[test]
fn bridge_metric_gauges_persist_across_restart() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("metrics.db");

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            let app = router(state);
            let payload = build_ingest_payload(17);
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        runtime::sleep(Duration::from_secs(1)).await;

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            let app = router(state);
            let payload = build_ingest_payload(53);
            let req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .unwrap()
                .build();
            let resp = app.handle(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let metrics_resp = app
                .handle(app.request_builder().path("/metrics").build())
                .await
                .unwrap();
            assert_eq!(metrics_resp.status(), StatusCode::OK);
            let body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
            let labels = r#"metric="bridge_reward_claims_total",peer="bridge-node",labels="""#;
            let delta = scrape_metric_value(&body, "bridge_metric_delta", labels)
                .expect("delta gauge present");
            let expected_delta = 53.0 - 17.0;
            assert!(
                (delta - expected_delta).abs() < 1e-6,
                "delta {delta} should equal {expected_delta}"
            );
            let rate = scrape_metric_value(&body, "bridge_metric_rate_per_second", labels)
                .expect("rate gauge present");
            assert!(rate > 0.0, "rate should be positive after restart");
            assert!(
                rate <= delta,
                "rate {rate} should not exceed delta {delta} when computed per second"
            );
        }
    });
}

#[test]
fn bridge_remediation_ack_latency_persists_across_restart() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        reset_bridge_remediation_ack_metrics();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("metrics.db");
        let response = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":true,"closed":false,"notes":"persist"}"#)
                .expect("ack response json"),
        );
        let (override_guard, captured) = install_http_override(response);
        let _override_guard = override_guard;
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");

        let baseline = [10u64, 12, 13, 15, 17, 20, 21];
        let mut initial_sample = None;

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            let shared_state = state.clone();
            let app = router(state);
            for value in baseline {
                let payload = build_labeled_payload(value, "eth");
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "token")
                    .json(&payload)
                    .unwrap()
                    .build();
                let resp = app.handle(req).await.unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                runtime::sleep(Duration::from_millis(20)).await;
            }

            let spike_payload = build_labeled_payload(200, "eth");
            let spike_req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&spike_payload)
                .unwrap()
                .build();
            let spike_resp = app.handle(spike_req).await.unwrap();
            assert_eq!(spike_resp.status(), StatusCode::OK);

            let observed = wait_for_requests(&captured, 1).await;
            assert!(observed >= 1, "expected acknowledgement dispatch");

            runtime::sleep(Duration::from_millis(150)).await;
            for _ in 0..60 {
                let observations = shared_state.bridge_ack_latency_observations();
                if let Some((_playbook, _state, latency, count)) =
                    observations.into_iter().find(|(playbook, state, _, _)| {
                        playbook == "governance-escalation" && state == "acknowledged"
                    })
                {
                    initial_sample = Some((latency, count));
                    break;
                }
                runtime::sleep(Duration::from_millis(100)).await;
            }
            assert!(
                initial_sample.is_some(),
                "ack latency sample recorded before restart"
            );
        }

        reset_bridge_remediation_ack_metrics();

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            let shared_state = state.clone();
            let app = router(state);
            let mut restored_sample = None;
            let mut metrics_snapshot = String::new();
            for _ in 0..60 {
                let observations = shared_state.bridge_ack_latency_observations();
                if let Some((_playbook, _state, latency, count)) =
                    observations.into_iter().find(|(playbook, state, _, _)| {
                        playbook == "governance-escalation" && state == "acknowledged"
                    })
                {
                    restored_sample = Some((latency, count));
                    let metrics_resp = app
                        .handle(app.request_builder().path("/metrics").build())
                        .await
                        .unwrap();
                    assert_eq!(metrics_resp.status(), StatusCode::OK);
                    metrics_snapshot = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
                    break;
                }
                runtime::sleep(Duration::from_millis(100)).await;
            }
            let (initial_latency, initial_count) =
                initial_sample.expect("ack latency sample captured before restart");
            let (restored_latency, restored_count) =
                restored_sample.expect("ack latency sample restored after restart");
            assert_eq!(restored_latency, initial_latency);
            assert_eq!(restored_count, initial_count);
            let labels = "playbook=\"governance-escalation\",state=\"acknowledged\"";
            let count_metric = scrape_metric_value(
                &metrics_snapshot,
                "bridge_remediation_ack_latency_seconds_count",
                labels,
            )
            .expect("ack latency count metric restored after restart");
            assert!(
                (count_metric - restored_count as f64).abs() < f64::EPSILON,
                "restored histogram count should match stored sample"
            );
            let bucket_labels =
                "playbook=\"governance-escalation\",state=\"acknowledged\",le=\"+Inf\"";
            let bucket = scrape_metric_value(
                &metrics_snapshot,
                "bridge_remediation_ack_latency_seconds_bucket",
                bucket_labels,
            )
            .expect("ack latency bucket restored after restart");
            assert!(bucket >= count_metric, "bucket should include all samples");
        }
    });
}

#[test]
fn bridge_remediation_cleans_spool_artifacts_after_restart() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        reset_bridge_remediation_ack_metrics();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("metrics.db");
        let spool_dir = tempfile::tempdir().unwrap();

        let response_pending = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":false,"notes":"pending"}"#)
                .expect("pending response json"),
        );
        let (override_guard, captured_initial) = install_http_override(response_pending);
        let override_guard = override_guard;
        let _throttle_guard = EnvGuard::set("TB_REMEDIATION_THROTTLE_URLS", "http://override/hook");
        let _escalate_url_guard =
            EnvGuard::set("TB_REMEDIATION_ESCALATE_URLS", "http://override/hook");
        let _escalate_dir_guard = EnvGuard::set(
            "TB_REMEDIATION_ESCALATE_DIRS",
            spool_dir.path().to_str().expect("spool path"),
        );
        let _retry_guard = EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS", "1");
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ACK_ESCALATE_SECS", "3");
        let _max_guard = EnvGuard::set("TB_REMEDIATION_ACK_MAX_RETRIES", "1");
        let _cleanup_guard = EnvGuard::set("AGGREGATOR_CLEANUP_INTERVAL_SECS", "1");

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            state.spawn_cleanup();
            let app = router(state);
            for value in [10u64, 11, 12, 13, 14, 15, 16] {
                let payload = build_labeled_payload(value, "eth");
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "token")
                    .json(&payload)
                    .unwrap()
                    .build();
                let resp = app.handle(req).await.unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                runtime::sleep(Duration::from_millis(20)).await;
            }

            let spike_payload = build_labeled_payload(220, "eth");
            let spike_req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&spike_payload)
                .unwrap()
                .build();
            let spike_resp = app.handle(spike_req).await.unwrap();
            assert_eq!(spike_resp.status(), StatusCode::OK);

            wait_for_requests(&captured_initial, 1).await;
            let mut observed = 0usize;
            for _ in 0..50 {
                observed = fs::read_dir(spool_dir.path())
                    .map(|iter| iter.count())
                    .unwrap_or(0);
                if observed > 0 {
                    break;
                }
                runtime::sleep(Duration::from_millis(40)).await;
            }
            assert!(observed > 0, "expected spool artifact to be persisted");

            let metrics_resp = app
                .handle(app.request_builder().path("/metrics").build())
                .await
                .unwrap();
            assert_eq!(metrics_resp.status(), StatusCode::OK);
            let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
            let gauge =
                scrape_metric_value(&metrics_body, "bridge_remediation_spool_artifacts", "")
                    .expect("spool gauge present after initial dispatch");
            assert!(
                (gauge - observed as f64).abs() < f64::EPSILON,
                "spool gauge {gauge} should equal observed artifact count {observed}"
            );
        }

        drop(override_guard);

        let response_closed = HookResponse::Json(
            json::value_from_str(r#"{"acknowledged":true,"closed":true,"notes":"cleared"}"#)
                .expect("closed response json"),
        );
        let (override_guard, captured_followup) = install_http_override(response_closed);
        let override_guard = override_guard;

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            state.spawn_cleanup();
            let app = router(state);

            wait_for_requests(&captured_followup, 1).await;

            for _ in 0..100 {
                let remaining = fs::read_dir(spool_dir.path())
                    .map(|iter| iter.count())
                    .unwrap_or(0);
                if remaining == 0 {
                    break;
                }
                runtime::sleep(Duration::from_millis(50)).await;
            }

            let remediation_resp = app
                .handle(app.request_builder().path("/remediation/bridge").build())
                .await
                .unwrap();
            let actions = parse_remediation_actions(remediation_resp.body());
            for action in actions {
                if action.acknowledged_at.is_some() || action.closed_out_at.is_some() {
                    assert!(
                        action.spool_artifacts.is_empty(),
                        "expected cleared action {} to drop spool artifacts",
                        action.metric
                    );
                }
            }

            let metrics_resp = app
                .handle(app.request_builder().path("/metrics").build())
                .await
                .unwrap();
            assert_eq!(metrics_resp.status(), StatusCode::OK);
            let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
            let gauge =
                scrape_metric_value(&metrics_body, "bridge_remediation_spool_artifacts", "")
                    .expect("spool gauge present after acknowledgement");
            assert!(
                (gauge - 0.0).abs() < f64::EPSILON,
                "spool gauge should reset to zero after acknowledgements"
            );
        }

        let final_count = fs::read_dir(spool_dir.path())
            .map(|iter| iter.count())
            .unwrap_or(0);
        assert_eq!(
            final_count, 0,
            "spool directory should be empty after restart cleanup"
        );

        drop(override_guard);
    });
}

#[test]
fn bridge_remediation_retains_spool_artifacts_after_retry_exhaustion() {
    run_async(async {
        reset_bridge_remediation_dispatch_log();
        reset_bridge_remediation_ack_metrics();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("metrics.db");
        let spool_dir = tempfile::tempdir().unwrap();

        let _escalate_dir_guard = EnvGuard::set(
            "TB_REMEDIATION_ESCALATE_DIRS",
            spool_dir.path().to_str().expect("spool path"),
        );
        let _retry_guard = EnvGuard::set("TB_REMEDIATION_ACK_RETRY_SECS", "1");
        let _escalate_guard = EnvGuard::set("TB_REMEDIATION_ACK_ESCALATE_SECS", "120");
        let _max_guard = EnvGuard::set("TB_REMEDIATION_ACK_MAX_RETRIES", "2");
        let _cleanup_guard = EnvGuard::set("AGGREGATOR_CLEANUP_INTERVAL_SECS", "1");

        let expected_artifacts = 3usize;

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            state.spawn_cleanup();
            let app = router(state);
            for value in [10u64, 11, 12, 13, 14, 15, 16] {
                let payload = build_labeled_payload(value, "eth");
                let req = app
                    .request_builder()
                    .method(Method::Post)
                    .path("/ingest")
                    .header("x-auth-token", "token")
                    .json(&payload)
                    .unwrap()
                    .build();
                let resp = app.handle(req).await.unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                runtime::sleep(Duration::from_millis(20)).await;
            }

            let spike_payload = build_labeled_payload(240, "eth");
            let spike_req = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&spike_payload)
                .unwrap()
                .build();
            let spike_resp = app.handle(spike_req).await.unwrap();
            assert_eq!(spike_resp.status(), StatusCode::OK);

            let mut observed = 0usize;
            for _ in 0..100 {
                observed = fs::read_dir(spool_dir.path())
                    .map(|iter| iter.count())
                    .unwrap_or(0);
                if observed >= expected_artifacts {
                    break;
                }
                runtime::sleep(Duration::from_millis(100)).await;
            }
            assert_eq!(
                observed, expected_artifacts,
                "expected spool artifacts after retry exhaustion"
            );

            let metrics_resp = app
                .handle(app.request_builder().path("/metrics").build())
                .await
                .unwrap();
            assert_eq!(metrics_resp.status(), StatusCode::OK);
            let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
            let gauge =
                scrape_metric_value(&metrics_body, "bridge_remediation_spool_artifacts", "")
                    .expect("spool gauge present during retry exhaustion");
            assert!(
                (gauge - expected_artifacts as f64).abs() < f64::EPSILON,
                "spool gauge {gauge} should equal artifact count {expected_artifacts}"
            );

            let remediation_resp = app
                .handle(app.request_builder().path("/remediation/bridge").build())
                .await
                .unwrap();
            let actions = parse_remediation_actions(remediation_resp.body());
            let pending = actions
                .into_iter()
                .find(|action| action.acknowledged_at.is_none() && action.closed_out_at.is_none())
                .expect("pending remediation action present");
            assert_eq!(
                pending.spool_artifacts.len(),
                expected_artifacts,
                "pending action should retain all spool artifacts"
            );
        }

        {
            let state = AppState::new("token".into(), db_path.clone(), 60);
            state.spawn_cleanup();
            let app = router(state);

            let metrics_resp = app
                .handle(app.request_builder().path("/metrics").build())
                .await
                .unwrap();
            assert_eq!(metrics_resp.status(), StatusCode::OK);
            let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).unwrap();
            let gauge =
                scrape_metric_value(&metrics_body, "bridge_remediation_spool_artifacts", "")
                    .expect("spool gauge present after restart with pending artifacts");
            assert!(
                (gauge - expected_artifacts as f64).abs() < f64::EPSILON,
                "spool gauge {gauge} should persist across restart"
            );

            let remaining = fs::read_dir(spool_dir.path())
                .map(|iter| iter.count())
                .unwrap_or(0);
            assert_eq!(
                remaining, expected_artifacts,
                "spool directory should still contain pending artifacts"
            );

            let remediation_resp = app
                .handle(app.request_builder().path("/remediation/bridge").build())
                .await
                .unwrap();
            let actions = parse_remediation_actions(remediation_resp.body());
            let pending = actions
                .into_iter()
                .find(|action| action.acknowledged_at.is_none() && action.closed_out_at.is_none())
                .expect("pending remediation action present after restart");
            assert_eq!(
                pending.spool_artifacts.len(),
                expected_artifacts,
                "pending action should keep spool artifacts after restart"
            );
        }
    });
}
