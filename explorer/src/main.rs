use axum::{extract::Path, routing::get, Json, Router};
use explorer::Explorer;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db_path = args.get(1).cloned().unwrap_or_else(|| "explorer.db".into());
    let receipts_dir = args.get(2).cloned();
    let explorer = Arc::new(Explorer::open(&db_path)?);
    if let Some(dir) = receipts_dir {
        explorer.ingest_dir(std::path::Path::new(&dir))?;
    }
    let state = explorer.clone();
    let app = Router::new()
        .route(
            "/receipts/provider/:id",
            get(move |Path(id): Path<String>| {
                let state = state.clone();
                async move { Json(state.receipts_by_provider(&id).unwrap_or_default()) }
            }),
        )
        .route(
            "/receipts/domain/:id",
            get(move |Path(id): Path<String>| {
                let state = explorer.clone();
                async move { Json(state.receipts_by_domain(&id).unwrap_or_default()) }
            }),
        );
    let listener = TcpListener::bind("0.0.0.0:3001").await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
