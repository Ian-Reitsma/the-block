use axum::body::{self, Body};
use axum::http::{Request, StatusCode};
use metrics_aggregator::{router, AppState};
use serde::Deserialize;
use serde_json::json;
use tempfile::tempdir;
use tower::ServiceExt;

#[derive(Deserialize)]
struct ApiCorrelation {
    correlation_id: String,
    peer_id: String,
    metric: String,
    value: Option<f64>,
    timestamp: u64,
}

#[tokio::test]
async fn indexes_correlation_entries() {
    let dir = tempdir().unwrap();
    let state = AppState::new("token".into(), dir.path().join("correlation.db"), 60);
    let app = router(state.clone());
    let payload = json!([{
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
    }]);
    let req = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("x-auth-token", "token")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/correlations/quic_handshake_fail_total")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let entries: Vec<ApiCorrelation> = serde_json::from_slice(&body).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].correlation_id, "abc123");
    assert_eq!(entries[0].peer_id, "peer-1");
    assert_eq!(entries[0].metric, "quic_handshake_fail_total");
    assert_eq!(entries[0].value, Some(2.0));
    assert!(entries[0].timestamp > 0);
}
