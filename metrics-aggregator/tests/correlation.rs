use foundation_serialization::{json, Deserialize};
use httpd::{Method, StatusCode};
use metrics_aggregator::{router, AppState};
use std::future::Future;
use sys::tempfile;

#[derive(Deserialize)]
struct ApiCorrelation {
    correlation_id: String,
    peer_id: String,
    metric: String,
    value: Option<f64>,
    timestamp: u64,
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
        let entries: Vec<ApiCorrelation> = json::from_slice(resp.body()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].correlation_id, "abc123");
        assert_eq!(entries[0].peer_id, "peer-1");
        assert_eq!(entries[0].metric, "quic_handshake_fail_total");
        assert_eq!(entries[0].value, Some(2.0));
        assert!(entries[0].timestamp > 0);
    });
}
