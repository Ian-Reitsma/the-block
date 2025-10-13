use http_env::server_tls_from_env;
use httpd::{serve, serve_tls, ServerConfig};
use metrics_aggregator::{install_tls_env_warning_forwarder, router, AppState};
use runtime::net::TcpListener;
use std::{env, net::SocketAddr, path::PathBuf};

fn main() {
    metrics_aggregator::ensure_foundation_metrics_recorder();
    install_tls_env_warning_forwarder();
    runtime::block_on(async {
        let addr: SocketAddr = env::var("AGGREGATOR_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:9000".into())
            .parse()
            .expect("invalid addr");
        let token = env::var("AGGREGATOR_TOKEN").unwrap_or_default();
        let db: PathBuf = env::var("AGGREGATOR_DB")
            .unwrap_or_else(|_| "peer_metrics.db".into())
            .into();
        let retention = env::var("AGGREGATOR_RETENTION_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24 * 60 * 60);
        let tls_warning_retention = env::var("AGGREGATOR_TLS_WARNING_RETENTION_SECS")
            .ok()
            .and_then(|s| s.parse().ok());
        let state =
            AppState::new_with_opts(token, None, &db, retention, None, tls_warning_retention);
        state.spawn_cleanup();
        let app = router(state);
        let listener = TcpListener::bind(addr).await.expect("bind listener");
        let config = ServerConfig::default();
        let tls = server_tls_from_env("TB_AGGREGATOR_TLS", Some("AGGREGATOR"))
            .unwrap_or_else(|err| panic!("metrics-aggregator: invalid TLS configuration: {err}"));
        if let Some(result) = tls {
            if result.legacy_env {
                eprintln!(
                    "metrics-aggregator: using legacy AGGREGATOR_* TLS variables; migrate to TB_AGGREGATOR_TLS_*",
                );
            }
            serve_tls(listener, app, config, result.config)
                .await
                .expect("serve tls");
        } else {
            serve(listener, app, config).await.expect("serve http");
        }
    });
}
