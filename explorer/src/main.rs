use diagnostics::{
    anyhow::{self, Result},
    Context,
};
use explorer::{router, Explorer, ExplorerHttpState};
use http_env::server_tls_from_env;
use httpd::{serve, serve_tls, ServerConfig};
use runtime::net::TcpListener;
use std::{env, net::SocketAddr, path::Path, sync::Arc};

fn main() -> Result<()> {
    runtime::block_on(async {
        let args: Vec<String> = env::args().collect();
        let db_path = args.get(1).cloned().unwrap_or_else(|| "explorer.db".into());
        let receipts_dir = args.get(2).cloned();

        let explorer = Arc::new(Explorer::open(&db_path).context("open explorer database")?);
        if let Some(dir) = receipts_dir {
            explorer
                .ingest_dir(Path::new(&dir))
                .with_context(|| format!("ingest receipts from {dir}"))?;
        }

        let addr: SocketAddr = env::var("EXPLORER_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:3001".into())
            .parse()
            .context("parse EXPLORER_ADDR")?;
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("bind explorer listener at {addr}"))?;
        let state = ExplorerHttpState::new(explorer);
        let app = router(state);
        let config = ServerConfig::default();

        let tls = server_tls_from_env("TB_EXPLORER_TLS", Some("EXPLORER"))
            .map_err(anyhow::Error::from_error)
            .context("load explorer TLS configuration")?;
        if let Some(result) = tls {
            if result.legacy_env {
                eprintln!(
                    "explorer: using legacy EXPLORER_* TLS variables; migrate to TB_EXPLORER_TLS_*",
                );
            }
            serve_tls(listener, app, config, result.config)
                .await
                .context("serve explorer tls")?;
        } else {
            serve(listener, app, config)
                .await
                .context("serve explorer http")?;
        }

        Ok(())
    })
}
