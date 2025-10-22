use foundation_serialization::json::{self, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{router, AppState};
use std::future::Future;
use std::time::Duration;
use sys::tempfile;

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

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
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
