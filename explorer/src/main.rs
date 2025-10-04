use anyhow::Context;
use explorer::{router, Explorer, ExplorerHttpState};
use httpd::{serve, serve_tls, ServerConfig, ServerTlsConfig};
use runtime::net::TcpListener;
use std::{env, net::SocketAddr, path::Path, sync::Arc};

fn main() -> anyhow::Result<()> {
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

        if let (Ok(cert), Ok(key)) = (env::var("EXPLORER_CERT"), env::var("EXPLORER_KEY")) {
            let tls = if let Ok(ca) = env::var("EXPLORER_CLIENT_CA") {
                ServerTlsConfig::from_pem_files_with_client_auth(cert, key, ca)
                    .context("explorer tls client auth config")?
            } else if let Ok(ca) = env::var("EXPLORER_CLIENT_CA_OPTIONAL") {
                ServerTlsConfig::from_pem_files_with_optional_client_auth(cert, key, ca)
                    .context("explorer optional client auth config")?
            } else {
                ServerTlsConfig::from_pem_files(cert, key).context("explorer tls config")?
            };
            serve_tls(listener, app, config, tls)
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
