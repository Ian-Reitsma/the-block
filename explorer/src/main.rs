use axum::{extract::Path, extract::Query, routing::get, Json, Router};
use explorer::{gov_param_view, Explorer};
use serde::Deserialize;
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
    let block_summary_state = explorer.clone();
    let tx_state = explorer.clone();
    let gov_state = explorer.clone();
    let rep_state = explorer.clone();
    let memo_state = explorer.clone();
    let contract_state = explorer.clone();
    let provider_state = explorer.clone();
    let domain_state = explorer.clone();
    let storage_provider_state = explorer.clone();
    let dex_state = explorer.clone();
    let job_state = explorer.clone();
    let trust_state = explorer.clone();
    let subsidy_state = explorer.clone();
    let metric_state = explorer.clone();
    let fee_floor_state = explorer.clone();
    let proof_state = explorer.clone();
    let handshake_state = explorer.clone();
    let token_state = explorer.clone();
    let bridge_state = explorer.clone();
    let juris_state = explorer.clone();
    let releases_query_state = explorer.clone();
    let identity_state = explorer.clone();
    let did_listing_state = explorer.clone();
    let did_rate_state = explorer.clone();

    #[derive(Deserialize, Default)]
    struct ReleaseQuery {
        page: Option<usize>,
        page_size: Option<usize>,
        proposer: Option<String>,
        start_epoch: Option<u64>,
        end_epoch: Option<u64>,
        store: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct GovHistoryQuery {
        store: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct DidQuery {
        address: Option<String>,
        limit: Option<usize>,
    }
    #[derive(Deserialize, Default)]
    struct RebateHistoryQuery {
        db: Option<String>,
        relayer: Option<String>,
        cursor: Option<u64>,
        limit: Option<usize>,
    }
    #[derive(Deserialize, Default)]
    struct RelayerBoardQuery {
        db: Option<String>,
        limit: Option<usize>,
    }
    let app = Router::new()
        .route(
            "/blocks/:hash",
            get(move |Path(hash): Path<String>| {
                let state = block_summary_state.clone();
                async move { Json(state.get_block(&hash).unwrap_or(None)) }
            }),
        )
        .route(
            "/blocks/:hash/summary",
            get(move |Path(hash): Path<String>| {
                let state = block_state.clone();
                async move {
                    if let Some(block) = state.get_block(&hash).unwrap_or(None) {
                        let summary =
                            explorer::summarize_block(block.index, block.transactions.len());
                        Json(Some(summary))
                    } else {
                        Json(None::<String>)
                    }
                }
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
            "/releases",
            get(move |Query(query): Query<ReleaseQuery>| {
                let cache = releases_query_state.clone();
                async move {
                    let gov_path = query.store.clone().unwrap_or_else(|| {
                        std::env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into())
                    });
                    let filter = explorer::ReleaseHistoryFilter {
                        proposer: query.proposer.clone(),
                        start_epoch: query.start_epoch,
                        end_epoch: query.end_epoch,
                    };
                    match explorer::paginated_release_history(
                        &gov_path,
                        query.page.unwrap_or(0),
                        query.page_size.unwrap_or(25),
                        filter,
                    ) {
                        Ok(page) => {
                            let _ = cache.record_release_entries(&page.entries);
                            Json(page)
                        }
                        Err(err) => {
                            eprintln!("release history query failed: {err}");
                            Json(explorer::ReleaseHistoryPage {
                                total: 0,
                                page: 0,
                                page_size: 0,
                                entries: Vec::new(),
                            })
                        }
                    }
                }
            }),
        )
        .route(
            "/light_client/top_relayers",
            get(move |Query(query): Query<RelayerBoardQuery>| {
                let db_path = query
                    .db
                    .clone()
                    .unwrap_or_else(|| "light_client/proof_rebates".into());
                let limit = query.limit.unwrap_or(10);
                async move {
                    match explorer::light_client::top_relayers(&db_path, limit) {
                        Ok(list) => Json(list),
                        Err(err) => {
                            eprintln!("top relayer query failed: {err}");
                            Json(Vec::<explorer::light_client::RelayerLeaderboardEntry>::new())
                        }
                    }
                }
            }),
        )
        .route(
            "/light_client/rebate_history",
            get(move |Query(query): Query<RebateHistoryQuery>| {
                let db_path = query
                    .db
                    .clone()
                    .unwrap_or_else(|| "light_client/proof_rebates".into());
                let relayer = query.relayer.clone();
                let cursor = query.cursor;
                let limit = query.limit.unwrap_or(25);
                async move {
                    match explorer::light_client::recent_rebate_history(
                        &db_path,
                        relayer.as_deref(),
                        cursor,
                        limit,
                    ) {
                        Ok(page) => Json(page),
                        Err(err) => {
                            eprintln!("rebate history query failed: {err}");
                            Json(explorer::light_client::RebateHistoryPage {
                                receipts: Vec::new(),
                                next: None,
                            })
                        }
                    }
                }
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
            "/dkg/validators",
            get(move || async move { Json(explorer::dkg_view::list_shares()) }),
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
            "/identity/dids/:address",
            get(move |Path(address): Path<String>| {
                let state = identity_state.clone();
                async move { Json(state.did_document(&address)) }
            }),
        )
        .route(
            "/dids",
            get(move |Query(query): Query<DidQuery>| {
                let state = did_listing_state.clone();
                async move {
                    if let Some(address) = query.address {
                        match explorer::did_view::by_address(&state, &address) {
                            Ok(rows) => Json(rows),
                            Err(err) => {
                                eprintln!("did history query failed: {err}");
                                Json(Vec::<explorer::DidRecordRow>::new())
                            }
                        }
                    } else {
                        let limit = query.limit.unwrap_or(25);
                        match explorer::did_view::recent(&state, limit) {
                            Ok(rows) => Json(rows),
                            Err(err) => {
                                eprintln!("recent did query failed: {err}");
                                Json(Vec::<explorer::DidRecordRow>::new())
                            }
                        }
                    }
                }
            }),
        )
        .route(
            "/dids/metrics/anchor_rate",
            get(move || {
                let state = did_rate_state.clone();
                async move { Json(explorer::did_view::anchor_rate(&state).unwrap_or_default()) }
            }),
        )
        .route(
            "/storage/providers",
            get(move || {
                let state = storage_provider_state.clone();
                async move { Json(state.provider_storage_stats().unwrap_or_default()) }
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
        )
        .route(
            "/dex/trust_lines",
            get(move || {
                let state = trust_state.clone();
                async move { Json(state.trust_lines().unwrap_or_default()) }
            }),
        )
        .route(
            "/subsidy/history",
            get(move || {
                let state = subsidy_state.clone();
                async move { Json(state.subsidy_history().unwrap_or_default()) }
            }),
        )
        .route(
            "/metrics/:name",
            get(move |Path(name): Path<String>| {
                let state = metric_state.clone();
                async move { Json(state.metric_points(&name).unwrap_or_default()) }
            }),
        )
        .route(
            "/mempool/fee_floor",
            get(move || {
                let state = fee_floor_state.clone();
                async move { Json(state.fee_floor_history().unwrap_or_default()) }
            }),
        )
        .route(
            "/mempool/fee_floor_policy",
            get(move |Query(query): Query<GovHistoryQuery>| {
                let store_path = query.store.unwrap_or_else(|| {
                    std::env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into())
                });
                async move {
                    match gov_param_view::fee_floor_policy_history(&store_path) {
                        Ok(records) => Json(records),
                        Err(err) => {
                            eprintln!("fee floor policy history failed: {err}");
                            Json(Vec::<gov_param_view::FeeFloorPolicyRecord>::new())
                        }
                    }
                }
            }),
        )
        .route(
            "/network/certs",
            get(move || async move { Json(explorer::net_view::list_peer_certs()) }),
        )
        .route(
            "/blocks/:hash/proof",
            get(move |Path(hash): Path<String>| {
                let state = proof_state.clone();
                async move { Json(state.light_proof(&hash).unwrap_or(None)) }
            }),
        )
        .route(
            "/peers/handshakes",
            get(move || {
                let state = handshake_state.clone();
                async move { Json(state.peer_handshakes().unwrap_or_default()) }
            }),
        )
        .route(
            "/tokens/supply/:symbol",
            get(move |Path(symbol): Path<String>| {
                let state = token_state.clone();
                async move { Json(state.token_supply(&symbol).unwrap_or_default()) }
            }),
        )
        .route(
            "/tokens/bridge/:symbol",
            get(move |Path(symbol): Path<String>| {
                let state = bridge_state.clone();
                async move { Json(state.bridge_volume(&symbol).unwrap_or_default()) }
            }),
        )
        .route(
            "/jurisdiction/:region",
            get(move |Path(region): Path<String>| {
                explorer::jurisdiction_view::route(juris_state.clone(), Path(region))
            }),
        );
    let listener = TcpListener::bind("0.0.0.0:3001").await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
