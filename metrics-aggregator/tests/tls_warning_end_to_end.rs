use foundation_serialization::json::Value;
use http_env::server_tls_from_env;
use httpd::{HttpClient, Method, ServerConfig};
use metrics_aggregator::{install_tls_env_warning_forwarder, router, AppState};
use runtime::net::TcpListener;
use runtime::{sleep, spawn};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos()
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
        let client = HttpClient::default();
        let ingest_url = format!("http://{addr}/ingest");

        let payload = foundation_serialization::json!([
            {
                "peer_id": "node-a",
                "metrics": {
                    "tls_env_warning_total": [
                        {"labels": {"prefix": ingest_prefix.as_str(), "code": ingest_code.as_str()}, "value": 1.0}
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
        assert_eq!(
            diagnostics_entry["variables"],
            foundation_serialization::json!([diag_key.as_str()])
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
