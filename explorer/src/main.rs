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
    let block_state = explorer.clone();
    let tx_state = explorer.clone();
    let gov_state = explorer.clone();
    let rep_state = explorer.clone();
    let memo_state = explorer.clone();
    let contract_state = explorer.clone();
    let provider_state = explorer.clone();
    let domain_state = explorer.clone();
    let dex_state = explorer.clone();
    let job_state = explorer.clone();
    let app = Router::new()
        .route(
            "/blocks/:hash",
            get(move |Path(hash): Path<String>| {
                let state = block_state.clone();
                async move { Json(state.get_block(&hash).unwrap_or(None)) }
            }),
        )
        .route(
            "/txs/:hash",
            get(move |Path(hash): Path<String>| {
                let state = tx_state.clone();
                async move { Json(state.get_tx(&hash).unwrap_or(None)) }
            }),
        )
        .route(
            "/gov/proposals/:id",
            get(move |Path(id): Path<u64>| {
                let state = gov_state.clone();
                async move { Json(state.get_gov_proposal(id).unwrap_or(None)) }
            }),
        )
        .route(
            "/peers/reputation",
            get(move || {
                let state = rep_state.clone();
                async move { Json(state.peer_reputations().unwrap_or_default()) }
            }),
        )
        .route(
            "/search/memo/:memo",
            get(move |Path(memo): Path<String>| {
                let state = memo_state.clone();
                async move { Json(state.search_memo(&memo).unwrap_or_default()) }
            }),
        )
        .route(
            "/search/contract/:contract",
            get(move |Path(contract): Path<String>| {
                let state = contract_state.clone();
                async move { Json(state.search_contract(&contract).unwrap_or_default()) }
            }),
        )
        .route(
            "/receipts/provider/:id",
            get(move |Path(id): Path<String>| {
                let state = provider_state.clone();
                async move { Json(state.receipts_by_provider(&id).unwrap_or_default()) }
            }),
        )
        .route(
            "/receipts/domain/:id",
            get(move |Path(id): Path<String>| {
                let state = domain_state.clone();
                async move { Json(state.receipts_by_domain(&id).unwrap_or_default()) }
            }),
        )
        .route(
            "/dex/order_book",
            get(move || {
                let state = dex_state.clone();
                async move { Json(state.order_book().unwrap_or_default()) }
            }),
        )
        .route(
            "/compute/jobs",
            get(move || {
                let state = job_state.clone();
                async move { Json(state.compute_jobs().unwrap_or_default()) }
            }),
        );
    let listener = TcpListener::bind("0.0.0.0:3001").await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
