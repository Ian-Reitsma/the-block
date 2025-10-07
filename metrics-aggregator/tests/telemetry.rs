use httpd::{Method, StatusCode};
use metrics_aggregator::{router, AppState};
use serde_json::json;
use std::future::Future;
use sys::tempfile;

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[test]
fn telemetry_round_trip() {
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
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

        let req = app
            .request_builder()
            .method(Method::Post)
            .path("/telemetry")
            .header("x-auth-token", "token")
            .json(&payload)
            .unwrap()
            .build();
        let resp = app.handle(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let resp = app
            .handle(app.request_builder().path("/telemetry").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let map: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert!(map.get("node-a").is_some());

        let resp = app
            .handle(app.request_builder().path("/telemetry/node-a").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let history: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);

        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.header("content-type"),
            Some("text/plain; version=0.0.4")
        );
        let body = String::from_utf8(resp.body().to_vec()).unwrap();
        assert!(
            body.contains("# TYPE aggregator_ingest_total counter"),
            "metrics payload missing ingest counter: {body}"
        );
        assert!(
            body.contains("# TYPE cluster_peer_active_total gauge"),
            "metrics payload missing active peer gauge: {body}"
        );
    });
}
