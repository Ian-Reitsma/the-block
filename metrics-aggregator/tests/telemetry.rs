use axum::body::{self, Body};
use axum::http::{Request, StatusCode};
use metrics_aggregator::{router, AppState};
use serde_json::json;
use std::future::Future;
use tempfile::tempdir;
use tower::ServiceExt;

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[test]
fn telemetry_round_trip() {
    run_async(async {
        let dir = tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state.clone());

        let payload = json!({
            "node_id": "node-a",
            "seq": 1,
            "timestamp": 1_700_000_000u64,
            "sample_rate_ppm": 500_000u64,
            "compaction_secs": 30u64,
            "memory": {
                "mempool": {"latest": 1024u64, "p50": 800u64, "p90": 900u64, "p99": 1000u64},
                "storage": {"latest": 2048u64, "p50": 1500u64, "p90": 1800u64, "p99": 1900u64},
                "compute": {"latest": 512u64, "p50": 400u64, "p90": 450u64, "p99": 500u64}
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/telemetry")
            .header("content-type", "application/json")
            .header("x-auth-token", "token")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let req = Request::builder()
            .uri("/telemetry")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let map: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(map.get("node-a").is_some());

        let req = Request::builder()
            .uri("/telemetry/node-a")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let history: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);
    });
}
