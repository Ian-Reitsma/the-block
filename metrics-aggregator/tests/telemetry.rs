use foundation_serialization::json::{self, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{metrics_registry_guard, router, AppState};
use runtime::telemetry::TEXT_MIME;
use std::future::Future;
use sys::tempfile;

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

async fn scrape_metrics(app: &httpd::Router<AppState>) -> String {
    let resp = app
        .handle(app.request_builder().path("/metrics").build())
        .await
        .expect("scrape metrics");
    String::from_utf8(resp.body().to_vec()).expect("metrics payload")
}

fn metric_value(metrics: &str, metric: &str) -> f64 {
    metrics
        .lines()
        .find_map(|line| {
            if !line.starts_with(metric) {
                return None;
            }
            let next = line.chars().nth(metric.len());
            if !matches!(next, Some(' ') | Some('{')) {
                return None;
            }
            line.split_whitespace().nth(1)?.parse::<f64>().ok()
        })
        .unwrap_or(0.0)
}

#[test]
fn telemetry_round_trip() {
    let _guard = metrics_registry_guard();
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state.clone());
        let baseline_metrics = scrape_metrics(&app).await;
        let baseline_ingest = metric_value(&baseline_metrics, "aggregator_telemetry_ingest_total");

        let payload = json::value_from_str(
            r#"{
                "node_id": "node-a",
                "seq": 1,
                "timestamp": 1700000000,
                "sample_rate_ppm": 500000,
                "compaction_secs": 30,
                "memory": {
                    "mempool": {"latest": 1024, "p50": 800, "p90": 900, "p99": 1000},
                    "storage": {"latest": 2048, "p50": 1500, "p90": 1800, "p99": 1900},
                    "compute": {"latest": 512, "p50": 400, "p90": 450, "p99": 500}
                },
                "ad_readiness": {
                    "ready": true,
                    "window_secs": 90,
                    "min_unique_viewers": 3,
                    "min_host_count": 2,
                    "min_provider_count": 1,
                    "unique_viewers": 8,
                    "host_count": 5,
                    "provider_count": 2,
                    "blockers": [],
                    "last_updated": 1700000001,
                    "total_usd_micros": 250000,
                    "settlement_count": 6,
                    "ct_price_usd_micros": 1250000,
                    "it_price_usd_micros": 990000,
                    "market_ct_price_usd_micros": 1300000,
                    "market_it_price_usd_micros": 995000,
                    "cohort_utilization": [
                        {
                            "domain": "example.test",
                            "provider": "edge-a",
                            "badges": ["premium"],
                            "price_per_mib_usd_micros": 120000,
                            "target_utilization_ppm": 900000,
                            "observed_utilization_ppm": 820000,
                            "delta_utilization_ppm": -80000
                        }
                    ],
                    "utilization_summary": {
                        "cohort_count": 1,
                        "mean_ppm": 820000,
                        "min_ppm": 820000,
                        "max_ppm": 820000,
                        "last_updated": 1700000002
                    }
                }
            }"#,
        )
        .unwrap();

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
        let map: Value = json::from_slice(resp.body()).unwrap();
        let summary = map.as_object().unwrap().get("node-a").unwrap();
        let readiness = summary.get("ad_readiness").unwrap();
        let cohorts = readiness
            .get("cohort_utilization")
            .unwrap()
            .as_array()
            .expect("cohort array");
        assert_eq!(
            cohorts[0]
                .get("delta_utilization_ppm")
                .and_then(Value::as_i64),
            Some(-80_000)
        );

        let resp = app
            .handle(app.request_builder().path("/telemetry/node-a").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let history: Value = json::from_slice(resp.body()).unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);

        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.header("content-type"), Some(TEXT_MIME));
        let body = String::from_utf8(resp.body().to_vec()).unwrap();
        assert!(
            body.contains("# TYPE aggregator_ingest_total counter"),
            "metrics payload missing ingest counter: {body}"
        );
        assert!(
            body.contains("# TYPE cluster_peer_active_total gauge"),
            "metrics payload missing active peer gauge: {body}"
        );
        let ingest_total = metric_value(&body, "aggregator_telemetry_ingest_total");
        assert!(
            ingest_total >= baseline_ingest + 1.0,
            "telemetry ingest counter did not advance: baseline={baseline_ingest}, current={ingest_total}"
        );
    });
}

#[test]
fn telemetry_rejects_schema_drift() {
    let _guard = metrics_registry_guard();
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state.clone());
        let baseline_metrics = scrape_metrics(&app).await;
        let baseline_schema_errors =
            metric_value(&baseline_metrics, "aggregator_telemetry_schema_error_total");

        let invalid = json::value_from_str(
            r#"{
                "node_id": "node-a",
                "seq": 1,
                "timestamp": 1700000000,
                "sample_rate_ppm": 500000,
                "compaction_secs": 30
            }"#,
        )
        .unwrap();

        let req = app
            .request_builder()
            .method(Method::Post)
            .path("/telemetry")
            .header("x-auth-token", "token")
            .json(&invalid)
            .unwrap()
            .build();
        let resp = app.handle(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: Value = json::from_slice(resp.body()).unwrap();
        let obj = body.as_object().expect("error body object");
        assert_eq!(obj.get("path").and_then(Value::as_str), Some("/memory"));
        assert!(obj
            .get("error")
            .and_then(Value::as_str)
            .unwrap()
            .contains("missing field"));

        let resp = app
            .handle(app.request_builder().path("/telemetry").build())
            .await
            .unwrap();
        let map: Value = json::from_slice(resp.body()).unwrap();
        assert!(map.as_object().unwrap().is_empty());

        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        let metrics = String::from_utf8(resp.body().to_vec()).unwrap();
        let schema_errors = metric_value(&metrics, "aggregator_telemetry_schema_error_total");
        assert!(
            schema_errors >= baseline_schema_errors + 1.0,
            "schema error counter did not advance: baseline={baseline_schema_errors}, current={schema_errors}"
        );
    });
}

#[test]
fn runtime_bridge_updates_foundation_metrics() {
    let _guard = metrics_registry_guard();
    run_async(async {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("metrics.db"), 60);
        let app = router(state);

        foundation_metrics::histogram!("runtime_spawn_latency_seconds", 0.05);
        foundation_metrics::gauge!("runtime_pending_tasks", 4.0);

        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(resp.body().to_vec()).unwrap();
        assert!(
            body.contains("runtime_spawn_latency_seconds_bucket"),
            "metrics payload missing runtime spawn histogram: {body}"
        );
        assert!(
            body.contains("runtime_pending_tasks 4"),
            "metrics payload missing runtime pending gauge: {body}"
        );
    });
}
