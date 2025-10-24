use foundation_serialization::json::{Map, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{router, AppState};
use runtime::block_on;
use sys::tempfile;

fn payout_sample(role: &str, value: f64) -> Value {
    let mut labels = Map::new();
    labels.insert("role".to_string(), Value::String(role.to_string()));
    let mut sample = Map::new();
    sample.insert("labels".to_string(), Value::Object(labels));
    sample.insert("value".to_string(), Value::from(value));
    Value::Object(sample)
}

fn build_payload(read: &[(&str, f64)], ad: &[(&str, f64)]) -> Value {
    let mut metrics = Map::new();
    metrics.insert(
        "explorer_block_payout_read_total".to_string(),
        Value::Array(
            read.iter()
                .map(|(role, value)| payout_sample(role, *value))
                .collect(),
        ),
    );
    metrics.insert(
        "explorer_block_payout_ad_total".to_string(),
        Value::Array(
            ad.iter()
                .map(|(role, value)| payout_sample(role, *value))
                .collect(),
        ),
    );
    let mut entry = Map::new();
    entry.insert("peer_id".to_string(), Value::String("explorer-node".into()));
    entry.insert("metrics".to_string(), Value::Object(metrics));
    Value::Array(vec![Value::Object(entry)])
}

fn scrape_metric(body: &str, metric: &str, role: &str) -> Option<f64> {
    let role_label = format!("role=\"{role}\"");
    body.lines().find_map(|line| {
        if !line.starts_with(metric) || !line.contains(&role_label) {
            return None;
        }
        line.split_whitespace().nth(1)?.parse::<f64>().ok()
    })
}

#[test]
fn explorer_payout_counters_increment_on_ingest() {
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let first_payload = build_payload(
            &[("viewer", 100.0), ("host", 50.0)],
            &[("viewer", 20.0), ("miner", 5.0)],
        );
        let request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&first_payload)
            .expect("serialize first payload")
            .build();
        let response = app.handle(request).await.expect("first ingest");
        assert_eq!(response.status(), StatusCode::OK);

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics scrape");
        let metrics_body = String::from_utf8(metrics_resp.body().to_vec()).expect("metrics body");
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_read_total", "viewer"),
            Some(100.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_read_total", "host"),
            Some(50.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "viewer"),
            Some(20.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "miner"),
            Some(5.0)
        );

        let second_payload = build_payload(
            &[("viewer", 130.0), ("host", 55.0)],
            &[("viewer", 35.0), ("miner", 7.0)],
        );
        let second_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&second_payload)
            .expect("serialize second payload")
            .build();
        let second_response = app.handle(second_request).await.expect("second ingest");
        assert_eq!(second_response.status(), StatusCode::OK);

        let updated_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics scrape after second ingest");
        let updated_metrics = String::from_utf8(updated_resp.body().to_vec())
            .expect("metrics body after second ingest");
        assert_eq!(
            scrape_metric(
                &updated_metrics,
                "explorer_block_payout_read_total",
                "viewer"
            ),
            Some(130.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_read_total", "host"),
            Some(55.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_ad_total", "viewer"),
            Some(35.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_ad_total", "miner"),
            Some(7.0)
        );
    });
}
