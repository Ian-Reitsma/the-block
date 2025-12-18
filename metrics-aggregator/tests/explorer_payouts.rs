use std::{collections::HashSet, time::Instant};

use foundation_serialization::json::{Map, Value};
use httpd::{Method, StatusCode};
use metrics_aggregator::{metrics_registry_guard, router, AppState};
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

fn scalar_metric(value: f64) -> Value {
    let mut sample = Map::new();
    sample.insert("value".to_string(), Value::from(value));
    Value::Object(sample)
}

fn build_payload(read: &[(&str, f64)], ad: &[(&str, f64)]) -> Value {
    build_payload_inner(
        read.iter()
            .map(|(role, value)| payout_sample(role, *value))
            .collect(),
        ad.iter()
            .map(|(role, value)| payout_sample(role, *value))
            .collect(),
    )
}

fn build_payload_owned(read: &[(String, f64)], ad: &[(String, f64)]) -> Value {
    build_payload_inner(
        read.iter()
            .map(|(role, value)| payout_sample(role, *value))
            .collect(),
        ad.iter()
            .map(|(role, value)| payout_sample(role, *value))
            .collect(),
    )
}

fn build_peer_entry(peer_id: &str, read_samples: Vec<Value>, ad_samples: Vec<Value>) -> Value {
    let mut metrics = Map::new();
    metrics.insert(
        "explorer_block_payout_read_total".to_string(),
        Value::Array(read_samples),
    );
    metrics.insert(
        "explorer_block_payout_ad_total".to_string(),
        Value::Array(ad_samples),
    );
    let mut entry = Map::new();
    entry.insert("peer_id".to_string(), Value::String(peer_id.to_string()));
    entry.insert("metrics".to_string(), Value::Object(metrics));
    Value::Object(entry)
}

fn build_peer_entry_from_tuples(peer_id: &str, read: &[(&str, f64)], ad: &[(&str, f64)]) -> Value {
    let read_samples = read
        .iter()
        .map(|(role, value)| payout_sample(role, *value))
        .collect();
    let ad_samples = ad
        .iter()
        .map(|(role, value)| payout_sample(role, *value))
        .collect();
    build_peer_entry(peer_id, read_samples, ad_samples)
}

fn build_payload_inner(read_samples: Vec<Value>, ad_samples: Vec<Value>) -> Value {
    Value::Array(vec![build_peer_entry(
        "explorer-node",
        read_samples,
        ad_samples,
    )])
}

fn build_multi_peer_payload(entries: &[(&str, &[(&str, f64)], &[(&str, f64)])]) -> Value {
    let payload: Vec<Value> = entries
        .iter()
        .map(|(peer_id, read, ad)| build_peer_entry_from_tuples(peer_id, read, ad))
        .collect();
    Value::Array(payload)
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

fn scrape_peer_metric(body: &str, metric: &str, peer: &str) -> Option<f64> {
    let peer_label = format!("peer=\"{peer}\"");
    body.lines().find_map(|line| {
        if !line.starts_with(metric) || !line.contains(&peer_label) {
            return None;
        }
        line.split_whitespace().nth(1)?.parse::<f64>().ok()
    })
}

fn extract_role_labels(body: &str, metric: &str) -> HashSet<String> {
    body.lines()
        .filter_map(|line| {
            if !line.starts_with(metric) {
                return None;
            }
            let labels_section = line.split('{').nth(1)?;
            let role_field = labels_section.split('}').next()?;
            let start = role_field.find("role=\"")? + "role=\"".len();
            let remainder = &role_field[start..];
            let end = remainder.find('"')?;
            Some(remainder[..end].to_string())
        })
        .collect()
}

#[test]
fn explorer_payout_counters_increment_on_ingest() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let baseline_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("baseline metrics scrape");
        let baseline_body =
            String::from_utf8(baseline_resp.body().to_vec()).expect("baseline body");
        let baseline_read_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "viewer")
                .unwrap_or(0.0);
        let baseline_read_host =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "host")
                .unwrap_or(0.0);
        let baseline_ad_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "viewer")
                .unwrap_or(0.0);
        let baseline_ad_miner =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "miner").unwrap_or(0.0);
        let baseline_read_last_seen = scrape_metric(
            &baseline_body,
            "explorer_block_payout_read_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);
        let baseline_ad_last_seen = scrape_metric(
            &baseline_body,
            "explorer_block_payout_ad_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);

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
            Some(baseline_read_viewer + 100.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 50.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 20.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "miner"),
            Some(baseline_ad_miner + 5.0)
        );
        let first_read_last_seen = scrape_metric(
            &metrics_body,
            "explorer_block_payout_read_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);
        let first_ad_last_seen = scrape_metric(
            &metrics_body,
            "explorer_block_payout_ad_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);
        assert!(first_read_last_seen >= baseline_read_last_seen);
        assert!(first_ad_last_seen >= baseline_ad_last_seen);

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
            Some(baseline_read_viewer + 130.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 55.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 35.0)
        );
        assert_eq!(
            scrape_metric(&updated_metrics, "explorer_block_payout_ad_total", "miner"),
            Some(baseline_ad_miner + 7.0)
        );
        let updated_read_last_seen = scrape_metric(
            &updated_metrics,
            "explorer_block_payout_read_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);
        let updated_ad_last_seen = scrape_metric(
            &updated_metrics,
            "explorer_block_payout_ad_last_seen_timestamp",
            "viewer",
        )
        .unwrap_or(0.0);
        assert!(updated_read_last_seen >= first_read_last_seen);
        assert!(updated_ad_last_seen >= first_ad_last_seen);
    });
}

#[test]
fn explorer_payout_summary_metrics_exposed() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let mut metrics = Map::new();
        metrics.insert(
            "explorer_block_payout_read_total".to_string(),
            Value::Array(vec![payout_sample("viewer", 5.0)]),
        );
        metrics.insert(
            "explorer_block_payout_ad_total".to_string(),
            Value::Array(vec![payout_sample("viewer", 7.0)]),
        );
        metrics.insert(
            "explorer_block_payout_ad_it_total".to_string(),
            Value::Array(vec![payout_sample("host", 3.0)]),
        );
        metrics.insert(
            "explorer_block_payout_ad_usd_total".to_string(),
            scalar_metric(64_000.0),
        );
        metrics.insert(
            "explorer_block_payout_ad_settlement_count".to_string(),
            scalar_metric(8.0),
        );
        metrics.insert(
            "explorer_block_payout_ad_ct_price_usd_micros".to_string(),
            scalar_metric(25_000.0),
        );
        metrics.insert(
            "explorer_block_payout_ad_it_price_usd_micros".to_string(),
            scalar_metric(50_000.0),
        );
        let mut entry = Map::new();
        entry.insert("peer_id".to_string(), Value::String("explorer-node".into()));
        entry.insert("metrics".to_string(), Value::Object(metrics));
        let payload = Value::Array(vec![Value::Object(entry)]);

        let ingest_req = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&payload)
            .expect("serialize payload")
            .build();
        let ingest_resp = app.handle(ingest_req).await.expect("ingest ok");
        assert_eq!(ingest_resp.status(), StatusCode::OK);

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics scrape");
        let body = String::from_utf8(metrics_resp.body().to_vec()).expect("metrics body");

        let usd_total =
            scrape_peer_metric(&body, "explorer_block_payout_ad_usd_total", "explorer-node")
                .expect("usd total metric");
        assert_eq!(usd_total, 64_000.0);
        let settlement_count = scrape_peer_metric(
            &body,
            "explorer_block_payout_ad_settlement_count",
            "explorer-node",
        )
        .expect("settlement count metric");
        assert_eq!(settlement_count, 8.0);
        let ct_price = scrape_peer_metric(
            &body,
            "explorer_block_payout_ad_ct_price_usd_micros",
            "explorer-node",
        )
        .expect("ct price metric");
        assert_eq!(ct_price, 25_000.0);
        let it_price = scrape_peer_metric(
            &body,
            "explorer_block_payout_ad_it_price_usd_micros",
            "explorer-node",
        )
        .expect("it price metric");
        assert_eq!(it_price, 50_000.0);
    })
}

#[test]
fn explorer_payout_counters_ignore_decreases() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let baseline_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("baseline metrics scrape");
        let baseline_body =
            String::from_utf8(baseline_resp.body().to_vec()).expect("baseline metrics body");
        let baseline_read_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "viewer")
                .unwrap_or(0.0);
        let baseline_read_host =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "host")
                .unwrap_or(0.0);
        let baseline_ad_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "viewer")
                .unwrap_or(0.0);
        let baseline_ad_miner =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "miner").unwrap_or(0.0);

        let first_payload = build_payload(
            &[("viewer", 120.0), ("host", 60.0)],
            &[("viewer", 18.0), ("miner", 4.0)],
        );
        let first_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&first_payload)
            .expect("serialize first payload")
            .build();
        let first_response = app.handle(first_request).await.expect("first ingest");
        assert_eq!(first_response.status(), StatusCode::OK);

        let second_payload = build_payload(
            &[("viewer", 150.0), ("host", 75.0)],
            &[("viewer", 25.0), ("miner", 6.0)],
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

        let second_metrics = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics after growth");
        let second_body =
            String::from_utf8(second_metrics.body().to_vec()).expect("metrics body after growth");
        assert_eq!(
            scrape_metric(&second_body, "explorer_block_payout_read_total", "viewer"),
            Some(baseline_read_viewer + 150.0)
        );
        assert_eq!(
            scrape_metric(&second_body, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 75.0)
        );
        assert_eq!(
            scrape_metric(&second_body, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 25.0)
        );
        assert_eq!(
            scrape_metric(&second_body, "explorer_block_payout_ad_total", "miner"),
            Some(baseline_ad_miner + 6.0)
        );

        let regression_payload = build_payload(
            &[("viewer", 90.0), ("host", 55.0)],
            &[("viewer", 10.0), ("miner", 3.0)],
        );
        let regression_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&regression_payload)
            .expect("serialize regression payload")
            .build();
        let regression_response = app
            .handle(regression_request)
            .await
            .expect("regression ingest");
        assert_eq!(regression_response.status(), StatusCode::OK);

        let regression_metrics = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics after regression");
        let regression_body = String::from_utf8(regression_metrics.body().to_vec())
            .expect("metrics body after regression");
        assert_eq!(
            scrape_metric(
                &regression_body,
                "explorer_block_payout_read_total",
                "viewer"
            ),
            Some(baseline_read_viewer + 150.0),
            "viewer read payout should not emit a negative delta",
        );
        assert_eq!(
            scrape_metric(&regression_body, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 75.0),
            "host read payout should remain at the peak value",
        );
        assert_eq!(
            scrape_metric(&regression_body, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 25.0),
            "viewer ad payout should keep the cached monotonic total",
        );
        assert_eq!(
            scrape_metric(&regression_body, "explorer_block_payout_ad_total", "miner"),
            Some(baseline_ad_miner + 6.0),
            "miner ad payout should remain unchanged after regression",
        );
    });
}

#[test]
fn explorer_payout_ingest_handles_high_role_cardinality() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let baseline_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("baseline metrics scrape");
        let baseline_body =
            String::from_utf8(baseline_resp.body().to_vec()).expect("baseline metrics body");
        let baseline_read_roles =
            extract_role_labels(&baseline_body, "explorer_block_payout_read_total");
        let baseline_ad_roles =
            extract_role_labels(&baseline_body, "explorer_block_payout_ad_total");

        let iterations = 20usize;
        let role_count = 128usize;
        let mut expected_read = Vec::new();
        let mut expected_ad = Vec::new();
        let start = Instant::now();

        for iteration in 0..iterations {
            let mut read_roles = Vec::with_capacity(role_count);
            let mut ad_roles = Vec::with_capacity(role_count);
            let base = 1_000.0 + iteration as f64 * 2.0;
            let ad_base = 5_000.0 + iteration as f64 * 3.0;
            for idx in 0..role_count {
                let role_value = base + idx as f64;
                read_roles.push((format!("read-role-{idx:03}"), role_value));
                let ad_value = ad_base + idx as f64;
                ad_roles.push((format!("ad-role-{idx:03}"), ad_value));
            }
            let payload = build_payload_owned(&read_roles, &ad_roles);
            let request = app
                .request_builder()
                .method(Method::Post)
                .path("/ingest")
                .header("x-auth-token", "token")
                .json(&payload)
                .expect("serialize high-cardinality payload")
                .build();
            let response = app.handle(request).await.expect("high-cardinality ingest");
            assert_eq!(response.status(), StatusCode::OK);

            expected_read = read_roles;
            expected_ad = ad_roles;
        }

        let elapsed = start.elapsed();
        let per_iteration = elapsed.as_secs_f64() / iterations as f64;
        assert!(
            per_iteration < 0.1,
            "high-cardinality ingest exceeded latency budget: {per_iteration:.6} s/iter"
        );

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics scrape after high-cardinality ingest");
        let metrics_body =
            String::from_utf8(metrics_resp.body().to_vec()).expect("metrics body string");

        let final_read_roles =
            extract_role_labels(&metrics_body, "explorer_block_payout_read_total");
        let final_ad_roles = extract_role_labels(&metrics_body, "explorer_block_payout_ad_total");

        let new_read_roles = final_read_roles.difference(&baseline_read_roles).count();
        assert_eq!(
            new_read_roles, role_count,
            "expected to observe all newly ingested read roles"
        );

        let new_ad_roles = final_ad_roles.difference(&baseline_ad_roles).count();
        assert_eq!(
            new_ad_roles, role_count,
            "expected to observe all newly ingested advertising roles"
        );

        for index in [0usize, role_count / 2, role_count - 1] {
            let (ref role, value) = expected_read[index];
            let baseline_value =
                scrape_metric(&baseline_body, "explorer_block_payout_read_total", role)
                    .unwrap_or(0.0);
            assert_eq!(
                scrape_metric(&metrics_body, "explorer_block_payout_read_total", role),
                Some(baseline_value + value),
                "read payout mismatch for {role}"
            );
        }

        for index in [0usize, role_count / 2, role_count - 1] {
            let (ref role, value) = expected_ad[index];
            let baseline_value =
                scrape_metric(&baseline_body, "explorer_block_payout_ad_total", role)
                    .unwrap_or(0.0);
            assert_eq!(
                scrape_metric(&metrics_body, "explorer_block_payout_ad_total", role),
                Some(baseline_value + value),
                "advertising payout mismatch for {role}"
            );
        }
    });
}

#[test]
fn explorer_payout_counters_remain_monotonic_with_role_churn() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let baseline_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("baseline metrics scrape");
        let baseline_body =
            String::from_utf8(baseline_resp.body().to_vec()).expect("baseline metrics body");
        let baseline_read_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "viewer")
                .unwrap_or(0.0);
        let baseline_read_host =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "host")
                .unwrap_or(0.0);
        let baseline_read_hardware = scrape_metric(
            &baseline_body,
            "explorer_block_payout_read_total",
            "hardware",
        )
        .unwrap_or(0.0);
        let baseline_ad_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "viewer")
                .unwrap_or(0.0);
        let baseline_ad_miner =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "miner").unwrap_or(0.0);
        let baseline_ad_hardware =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "hardware")
                .unwrap_or(0.0);
        let baseline_ad_liquidity = scrape_metric(
            &baseline_body,
            "explorer_block_payout_ad_total",
            "liquidity",
        )
        .unwrap_or(0.0);

        let first_payload = build_payload(
            &[("viewer", 200.0), ("host", 80.0)],
            &[("viewer", 50.0), ("miner", 10.0), ("liquidity", 5.0)],
        );
        let first_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&first_payload)
            .expect("serialize first payload")
            .build();
        let first_response = app.handle(first_request).await.expect("first ingest");
        assert_eq!(first_response.status(), StatusCode::OK);

        let second_payload = build_payload(
            &[("viewer", 250.0), ("hardware", 40.0)],
            &[("viewer", 55.0), ("hardware", 15.0), ("liquidity", 3.0)],
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

        let regression_payload = build_payload(
            &[("viewer", 240.0), ("host", 70.0)],
            &[("viewer", 40.0), ("miner", 8.0), ("hardware", 10.0)],
        );
        let regression_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&regression_payload)
            .expect("serialize regression payload")
            .build();
        let regression_response = app
            .handle(regression_request)
            .await
            .expect("regression ingest");
        assert_eq!(regression_response.status(), StatusCode::OK);

        let rebound_payload = build_payload(
            &[("viewer", 260.0), ("host", 75.0), ("hardware", 42.0)],
            &[
                ("viewer", 70.0),
                ("miner", 12.0),
                ("hardware", 18.0),
                ("liquidity", 7.0),
            ],
        );
        let rebound_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&rebound_payload)
            .expect("serialize rebound payload")
            .build();
        let rebound_response = app.handle(rebound_request).await.expect("rebound ingest");
        assert_eq!(rebound_response.status(), StatusCode::OK);

        let metrics_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics scrape after churn");
        let metrics_body =
            String::from_utf8(metrics_resp.body().to_vec()).expect("metrics body after churn");

        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_read_total", "viewer"),
            Some(baseline_read_viewer + 260.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 80.0),
            "host totals should stay pinned to the peak despite later regressions",
        );
        assert_eq!(
            scrape_metric(
                &metrics_body,
                "explorer_block_payout_read_total",
                "hardware"
            ),
            Some(baseline_read_hardware + 42.0),
        );

        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 70.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "miner"),
            Some(baseline_ad_miner + 12.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "hardware"),
            Some(baseline_ad_hardware + 18.0)
        );
        assert_eq!(
            scrape_metric(&metrics_body, "explorer_block_payout_ad_total", "liquidity"),
            Some(baseline_ad_liquidity + 7.0)
        );

        let read_roles = extract_role_labels(&metrics_body, "explorer_block_payout_read_total");
        assert!(read_roles.contains("viewer"));
        assert!(read_roles.contains("host"));
        assert!(read_roles.contains("hardware"));

        let ad_roles = extract_role_labels(&metrics_body, "explorer_block_payout_ad_total");
        assert!(ad_roles.contains("viewer"));
        assert!(ad_roles.contains("miner"));
        assert!(ad_roles.contains("hardware"));
        assert!(ad_roles.contains("liquidity"));
    });
}

#[test]
fn explorer_payout_counters_are_peer_scoped() {
    let _guard = metrics_registry_guard();
    block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state);

        let baseline_resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("baseline metrics scrape");
        let baseline_body =
            String::from_utf8(baseline_resp.body().to_vec()).expect("baseline metrics body");
        let baseline_read_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "viewer")
                .unwrap_or(0.0);
        let baseline_read_host =
            scrape_metric(&baseline_body, "explorer_block_payout_read_total", "host")
                .unwrap_or(0.0);
        let baseline_ad_viewer =
            scrape_metric(&baseline_body, "explorer_block_payout_ad_total", "viewer")
                .unwrap_or(0.0);

        let first_payload = build_multi_peer_payload(&[(
            "explorer-alpha",
            &[("viewer", 120.0), ("host", 60.0)],
            &[("viewer", 30.0)],
        )]);
        let first_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&first_payload)
            .expect("serialize first payload")
            .build();
        let first_response = app.handle(first_request).await.expect("first ingest");
        assert_eq!(first_response.status(), StatusCode::OK);

        let after_first = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics after first ingest");
        let body_first = String::from_utf8(after_first.body().to_vec())
            .expect("metrics body after first ingest");
        assert_eq!(
            scrape_metric(&body_first, "explorer_block_payout_read_total", "viewer"),
            Some(baseline_read_viewer + 120.0)
        );
        assert_eq!(
            scrape_metric(&body_first, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 60.0)
        );
        assert_eq!(
            scrape_metric(&body_first, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 30.0)
        );

        let second_payload = build_multi_peer_payload(&[(
            "explorer-beta",
            &[("viewer", 80.0)],
            &[("viewer", 15.0)],
        )]);
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

        let after_second = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics after second ingest");
        let body_second = String::from_utf8(after_second.body().to_vec())
            .expect("metrics body after second ingest");
        assert_eq!(
            scrape_metric(&body_second, "explorer_block_payout_read_total", "viewer"),
            Some(baseline_read_viewer + 200.0)
        );
        assert_eq!(
            scrape_metric(&body_second, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 60.0)
        );
        assert_eq!(
            scrape_metric(&body_second, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 45.0)
        );

        let third_payload = build_multi_peer_payload(&[
            (
                "explorer-alpha",
                &[("viewer", 115.0), ("host", 75.0)],
                &[("viewer", 28.0)],
            ),
            ("explorer-beta", &[("viewer", 90.0)], &[("viewer", 25.0)]),
        ]);
        let third_request = app
            .request_builder()
            .method(Method::Post)
            .path("/ingest")
            .header("x-auth-token", "token")
            .json(&third_payload)
            .expect("serialize third payload")
            .build();
        let third_response = app.handle(third_request).await.expect("third ingest");
        assert_eq!(third_response.status(), StatusCode::OK);

        let final_metrics = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics after third ingest");
        let final_body =
            String::from_utf8(final_metrics.body().to_vec()).expect("final metrics body");
        assert_eq!(
            scrape_metric(&final_body, "explorer_block_payout_read_total", "viewer"),
            Some(baseline_read_viewer + 210.0)
        );
        assert_eq!(
            scrape_metric(&final_body, "explorer_block_payout_read_total", "host"),
            Some(baseline_read_host + 75.0)
        );
        assert_eq!(
            scrape_metric(&final_body, "explorer_block_payout_ad_total", "viewer"),
            Some(baseline_ad_viewer + 55.0)
        );

        let viewer_roles = extract_role_labels(&final_body, "explorer_block_payout_read_total");
        assert!(viewer_roles.contains("viewer"));
        assert!(viewer_roles.contains("host"));

        let ad_roles = extract_role_labels(&final_body, "explorer_block_payout_ad_total");
        assert!(ad_roles.contains("viewer"));
    });
}
