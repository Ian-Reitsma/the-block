use crypto_suite::hashing::blake3;
use foundation_serialization::json::Value;
use http_env::server_tls_from_env;
use httpd::{HttpClient, Method, ServerConfig};
use metrics_aggregator::{install_tls_env_warning_forwarder, router, AppState};
use runtime::net::TcpListener;
use runtime::{sleep, spawn};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TLS_WARNING_FINGERPRINT_DELIM: u8 = 0x1f;

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos()
}

fn fingerprint(bytes: &[u8]) -> i64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&blake3::hash(bytes).as_bytes()[..8]);
    i64::from_le_bytes(buf)
}

fn fingerprint_hex(value: i64) -> String {
    let unsigned = u64::from_le_bytes(value.to_le_bytes());
    format!("{unsigned:016x}")
}

fn variables_fingerprint(variables: &[String]) -> i64 {
    let mut bytes = Vec::new();
    for (idx, value) in variables.iter().enumerate() {
        if idx > 0 {
            bytes.push(TLS_WARNING_FINGERPRINT_DELIM);
        }
        bytes.extend_from_slice(value.as_bytes());
    }
    fingerprint(&bytes)
}

#[test]
fn diagnostics_warnings_surface_over_http() {
    install_tls_env_warning_forwarder();

    runtime::block_on(async {
        let tempdir = sys::tempfile::tempdir().expect("create tempdir");
        let db_path = tempdir.path().join("metrics.db");
        let state = AppState::new("token".into(), &db_path, 60);
        let app = router(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");

        let server = spawn(async move {
            httpd::serve(listener, app, ServerConfig::default())
                .await
                .expect("serve aggregator");
        });

        sleep(Duration::from_millis(50)).await;

        let diag_prefix = format!("TB_TEST_TLS_{}", unique_suffix());
        let diag_code = "missing_identity_component";
        let diag_cert = format!("{diag_prefix}_CERT");
        let diag_key = format!("{diag_prefix}_KEY");
        std::env::set_var(&diag_cert, "/tmp/test-diag-cert.pem");
        std::env::remove_var(&diag_key);
        std::env::remove_var(format!("{diag_prefix}_CLIENT_CA"));
        std::env::remove_var(format!("{diag_prefix}_CLIENT_CA_OPTIONAL"));
        let _ = server_tls_from_env(&diag_prefix, None);

        let ingest_prefix = format!("TB_NODE_TLS_{}", unique_suffix());
        let ingest_code = format!("missing_anchor_{}", unique_suffix());
        let ingest_detail_hint = format!("{}:{}", ingest_prefix, ingest_code);
        let ingest_detail_fp = fingerprint(ingest_detail_hint.as_bytes());
        let ingest_variables = vec!["missing_anchor".to_string()];
        let ingest_variables_fp = variables_fingerprint(&ingest_variables);
        let ingest_detail_fp_metric = ingest_detail_fp;
        let ingest_variables_fp_metric = ingest_variables_fp;
        let client = HttpClient::default();
        let ingest_url = format!("http://{addr}/ingest");

        let payload = foundation_serialization::json!([
            {
                "peer_id": "node-a",
                "metrics": {
                    "tls_env_warning_total": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": 1.0}
                    ],
                    "tls_env_warning_detail_fingerprint": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": ingest_detail_fp}
                    ],
                    "tls_env_warning_variables_fingerprint": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": ingest_variables_fp}
                    ]
                }
            }
        ]);
        let response = client
            .request(Method::Post, &ingest_url)
            .expect("build ingest request")
            .header("x-auth-token", "token")
            .json(&payload)
            .expect("serialize ingest payload")
            .send()
            .await
            .expect("send ingest payload");
        assert_eq!(response.status(), httpd::StatusCode::OK);

        let payload = foundation_serialization::json!([
            {
                "peer_id": "node-a",
                "metrics": {
                    "tls_env_warning_total": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": 4.0}
                    ],
                    "tls_env_warning_detail_fingerprint": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": ingest_detail_fp}
                    ],
                    "tls_env_warning_variables_fingerprint": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": ingest_variables_fp}
                    ]
                }
            }
        ]);
        let response = client
            .request(Method::Post, &ingest_url)
            .expect("build second ingest request")
            .header("x-auth-token", "token")
            .json(&payload)
            .expect("serialize second ingest payload")
            .send()
            .await
            .expect("send second ingest payload");
        assert_eq!(response.status(), httpd::StatusCode::OK);

        let latest_url = format!("http://{addr}/tls/warnings/latest");
        let response = client
            .request(Method::Get, &latest_url)
            .expect("build latest request")
            .send()
            .await
            .expect("fetch latest warnings");
        assert_eq!(response.status(), httpd::StatusCode::OK);
        let snapshots: Vec<Value> = response.json().expect("parse latest warnings");

        let diagnostics_entry = snapshots
            .iter()
            .find(|entry| entry["prefix"] == foundation_serialization::json!(diag_prefix.as_str()))
            .expect("diagnostics warning present");
        assert_eq!(
            diagnostics_entry["code"],
            foundation_serialization::json!(diag_code)
        );
        assert_eq!(
            diagnostics_entry["total"],
            foundation_serialization::json!(1)
        );
        assert_eq!(
            diagnostics_entry["origin"],
            foundation_serialization::json!("diagnostics")
        );
        let detail = diagnostics_entry["detail"].as_str().expect("detail string");
        assert!(detail.contains(&diag_key));
        let detail_fingerprint = diagnostics_entry["detail_fingerprint"]
            .as_i64()
            .expect("detail fingerprint");
        assert_eq!(detail_fingerprint, fingerprint(detail.as_bytes()));
        assert_eq!(
            diagnostics_entry["variables"],
            foundation_serialization::json!([diag_key.as_str()])
        );
        let diag_variables: Vec<String> = diagnostics_entry["variables"]
            .as_array()
            .expect("variables array")
            .iter()
            .filter_map(|value| value.as_str().map(|s| s.to_string()))
            .collect();
        let variables_fingerprint_value = diagnostics_entry["variables_fingerprint"]
            .as_i64()
            .expect("variables fingerprint");
        assert_eq!(
            variables_fingerprint_value,
            variables_fingerprint(&diag_variables)
        );
        let detail_counts = diagnostics_entry["detail_fingerprint_counts"]
            .as_object()
            .expect("detail fingerprint counts object");
        assert_eq!(
            detail_counts
                .get(&fingerprint_hex(detail_fingerprint))
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        let variables_counts = diagnostics_entry["variables_fingerprint_counts"]
            .as_object()
            .expect("variables fingerprint counts object");
        assert_eq!(
            variables_counts
                .get(&fingerprint_hex(variables_fingerprint_value))
                .and_then(|value| value.as_u64()),
            Some(1)
        );

        let ingest_entry = snapshots
            .iter()
            .find(|entry| {
                entry["prefix"] == foundation_serialization::json!(ingest_prefix.as_str())
            })
            .expect("ingest warning present");
        assert_eq!(
            ingest_entry["code"],
            foundation_serialization::json!(ingest_code.as_str())
        );
        assert_eq!(ingest_entry["total"], foundation_serialization::json!(4));
        assert_eq!(
            ingest_entry["last_delta"],
            foundation_serialization::json!(3)
        );
        assert_eq!(
            ingest_entry["origin"],
            foundation_serialization::json!("peer_ingest")
        );
        assert_eq!(
            ingest_entry["peer_id"],
            foundation_serialization::json!("node-a")
        );
        assert_eq!(
            ingest_entry["detail_fingerprint"],
            foundation_serialization::json!(ingest_detail_fp_metric)
        );
        assert_eq!(
            ingest_entry["variables_fingerprint"],
            foundation_serialization::json!(ingest_variables_fp_metric)
        );
        let ingest_detail_hex = fingerprint_hex(ingest_detail_fp_metric);
        let ingest_variables_hex = fingerprint_hex(ingest_variables_fp_metric);
        let ingest_detail_counts = ingest_entry["detail_fingerprint_counts"]
            .as_object()
            .expect("ingest detail fingerprint counts");
        assert_eq!(
            ingest_detail_counts
                .get(&ingest_detail_hex)
                .and_then(|value| value.as_u64()),
            Some(4)
        );
        let ingest_variables_counts = ingest_entry["variables_fingerprint_counts"]
            .as_object()
            .expect("ingest variables fingerprint counts");
        assert_eq!(
            ingest_variables_counts
                .get(&ingest_variables_hex)
                .and_then(|value| value.as_u64()),
            Some(4)
        );

        let metrics_url = format!("http://{addr}/metrics");
        let response = client
            .request(Method::Get, &metrics_url)
            .expect("build metrics request")
            .send()
            .await
            .expect("fetch metrics snapshot");
        assert_eq!(response.status(), httpd::StatusCode::OK);
        let body = response.text().expect("metrics text payload");
        assert!(body.contains(&format!(
            "tls_env_warning_total{{prefix=\"{}\",code=\"{}\"}} 1",
            diag_prefix, diag_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_total{{prefix=\"{}\",code=\"{}\"}} 4",
            ingest_prefix, ingest_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_last_seen_seconds{{prefix=\"{}\",code=\"{}\"}}",
            diag_prefix, diag_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_last_seen_seconds{{prefix=\"{}\",code=\"{}\"}}",
            ingest_prefix, ingest_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_fingerprint{{prefix=\"{}\",code=\"{}\"}} {}",
            diag_prefix,
            diag_code,
            fingerprint(detail.as_bytes())
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_fingerprint{{prefix=\"{}\",code=\"{}\"}} {}",
            ingest_prefix, ingest_code, ingest_detail_fp_metric
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_fingerprint{{prefix=\"{}\",code=\"{}\"}} {}",
            diag_prefix,
            diag_code,
            variables_fingerprint(&diag_variables)
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_fingerprint{{prefix=\"{}\",code=\"{}\"}} {}",
            ingest_prefix, ingest_code, ingest_variables_fp_metric
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_unique_fingerprints{{prefix=\"{}\",code=\"{}\"}} 1",
            diag_prefix, diag_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_unique_fingerprints{{prefix=\"{}\",code=\"{}\"}} 1",
            ingest_prefix, ingest_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_unique_fingerprints{{prefix=\"{}\",code=\"{}\"}} 1",
            diag_prefix, diag_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_unique_fingerprints{{prefix=\"{}\",code=\"{}\"}} 1",
            ingest_prefix, ingest_code
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_fingerprint_total{{prefix=\"{}\",code=\"{}\",fingerprint=\"{}\"}} 1",
            diag_prefix,
            diag_code,
            fingerprint_hex(detail_fingerprint)
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_detail_fingerprint_total{{prefix=\"{}\",code=\"{}\",fingerprint=\"{}\"}} 4",
            ingest_prefix,
            ingest_code,
            ingest_detail_hex
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_fingerprint_total{{prefix=\"{}\",code=\"{}\",fingerprint=\"{}\"}} 1",
            diag_prefix,
            diag_code,
            fingerprint_hex(variables_fingerprint_value)
        )));
        assert!(body.contains(&format!(
            "tls_env_warning_variables_fingerprint_total{{prefix=\"{}\",code=\"{}\",fingerprint=\"{}\"}} 4",
            ingest_prefix,
            ingest_code,
            ingest_variables_hex
        )));

        let status_url = format!("http://{addr}/tls/warnings/status");
        let response = client
            .request(Method::Get, &status_url)
            .expect("build status request")
            .send()
            .await
            .expect("fetch status snapshot");
        assert_eq!(response.status(), httpd::StatusCode::OK);
        let status: Value = response.json().expect("parse status payload");
        let retention = status["retention_seconds"]
            .as_u64()
            .expect("retention seconds");
        assert!(retention >= 7 * 24 * 60 * 60);
        assert_eq!(status["active_snapshots"].as_u64(), Some(2));
        assert_eq!(status["stale_snapshots"].as_u64(), Some(0));
        assert!(status["most_recent_last_seen"].as_u64().is_some());
        assert!(status["least_recent_last_seen"].as_u64().is_some());

        server.abort();
        std::env::remove_var(diag_cert);
        std::env::remove_var(diag_key);
    });
}
