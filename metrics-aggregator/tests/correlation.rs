use foundation_serialization::json::{self, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{router, AppState};
use std::future::Future;
use sys::tempfile;

struct ApiCorrelation {
    correlation_id: String,
    peer_id: String,
    metric: String,
    value: Option<f64>,
    timestamp: u64,
}

fn parse_correlation_records(bytes: &[u8]) -> Vec<ApiCorrelation> {
    let value: Value = json::from_slice(bytes).expect("correlation response json");
    let array = value.as_array().expect("response array");
    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().expect("correlation object");
            ApiCorrelation {
                correlation_id: object
                    .get("correlation_id")
                    .and_then(Value::as_str)
                    .expect("correlation_id")
                    .to_string(),
                peer_id: object
                    .get("peer_id")
                    .and_then(Value::as_str)
                    .expect("peer_id")
                    .to_string(),
                metric: object
                    .get("metric")
                    .and_then(Value::as_str)
                    .expect("metric")
                    .to_string(),
                value: object.get("value").and_then(Value::as_f64),
                timestamp: object
                    .get("timestamp")
                    .and_then(Value::as_u64)
                    .expect("timestamp"),
            }
        })
        .collect()
}

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[test]
fn indexes_correlation_entries() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("correlation.db"), 60);
        let app = router(state.clone());
        let payload = json::value_from_str(
            r#"[
                {
                    "peer_id": "peer-1",
                    "metrics": {
                        "quic_handshake_fail_total": [
                            {"labels": {"correlation_id": "abc123"}, "value": 2.0}
                        ],
                        "anomaly_metric": {
                            "labels": {"correlation_id": "def456"},
                            "value": 1.0
                        }
                    }
                }
            ]"#,
        )
        .unwrap();
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

        let resp = app
            .handle(
                app.request_builder()
                    .path("/correlations/quic_handshake_fail_total")
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let entries = parse_correlation_records(resp.body());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].correlation_id, "abc123");
        assert_eq!(entries[0].peer_id, "peer-1");
        assert_eq!(entries[0].metric, "quic_handshake_fail_total");
        assert_eq!(entries[0].value, Some(2.0));
        assert!(entries[0].timestamp > 0);
    });
}
