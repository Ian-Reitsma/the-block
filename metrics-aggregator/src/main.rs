use httpd::{serve, serve_tls, ServerConfig, ServerTlsConfig};
use metrics_aggregator::{router, AppState};
use runtime::net::TcpListener;
use std::{env, net::SocketAddr, path::PathBuf};

fn main() {
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
        let state = AppState::new(token, db, retention);
        state.spawn_cleanup();
        let app = router(state);
        let listener = TcpListener::bind(addr).await.expect("bind listener");
        let config = ServerConfig::default();
        if let (Ok(cert), Ok(key)) = (env::var("AGGREGATOR_CERT"), env::var("AGGREGATOR_KEY")) {
            let tls = if let Ok(ca) = env::var("AGGREGATOR_CLIENT_CA") {
                ServerTlsConfig::from_pem_files_with_client_auth(cert, key, ca)
                    .expect("tls client auth config")
            } else if let Ok(ca) = env::var("AGGREGATOR_CLIENT_CA_OPTIONAL") {
                ServerTlsConfig::from_pem_files_with_optional_client_auth(cert, key, ca)
                    .expect("tls optional client auth config")
            } else {
                ServerTlsConfig::from_pem_files(cert, key).expect("tls config")
            };
            serve_tls(listener, app, config, tls)
                .await
                .expect("serve tls");
        } else {
            serve(listener, app, config).await.expect("serve http");
        }
    });
}
