use axum_server::tls_rustls::RustlsConfig;
use metrics_aggregator::{router, AppState};
use std::{env, net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() {
    let addr: SocketAddr = env::var("AGGREGATOR_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9000".into())
        .parse()
        .expect("invalid addr");
    let token = env::var("AGGREGATOR_TOKEN").unwrap_or_default();
    let db: PathBuf = env::var("AGGREGATOR_DB")
        .unwrap_or_else(|_| "peer_metrics.json".into())
        .into();
    let retention = env::var("AGGREGATOR_RETENTION_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24 * 60 * 60);
    let state = AppState::new(token, db, retention);
    let app = router(state);
    if let (Ok(cert), Ok(key)) = (env::var("AGGREGATOR_CERT"), env::var("AGGREGATOR_KEY")) {
        let config = RustlsConfig::from_pem_file(cert, key)
            .await
            .expect("tls config");
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    } else {
        axum_server::bind(addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    }
}
