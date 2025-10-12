use foundation_serialization::json::Value;
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
        let diag_code = format!("missing_key_{}", unique_suffix());
        diagnostics::warn!(
            target: "http_env.tls_env",
            prefix = %diag_prefix,
            code = diag_code.as_str(),
            detail = %"test diagnostics path",
            variables = ?vec!["a.pem", "b.pem"],
            "tls_env_warning"
        );

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
            foundation_serialization::json!(diag_code.as_str())
        );
        assert_eq!(
            diagnostics_entry["total"],
            foundation_serialization::json!(1)
        );
        assert_eq!(
            diagnostics_entry["origin"],
            foundation_serialization::json!("diagnostics")
        );
        assert_eq!(
            diagnostics_entry["detail"],
            foundation_serialization::json!("test diagnostics path")
        );
        assert_eq!(
            diagnostics_entry["variables"],
            foundation_serialization::json!(["a.pem", "b.pem"])
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

        server.abort();
    });
}
