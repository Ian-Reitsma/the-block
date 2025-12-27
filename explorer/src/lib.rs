use crate::ad_view::{
    detail_to_json, list_policy_snapshots, load_param_history, load_readiness_status,
    read_policy_snapshot, readiness_to_json, summary_to_json, AdPolicySnapshotDetail,
    AdPolicySnapshotSummary, AdReadinessStatusView,
};
use concurrency::cache::LruCache;
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex::{self, encode as hex_encode};
use diagnostics::anyhow::{self, Result as AnyhowResult};
use foundation_serialization::{binary, de::DeserializeOwned, json, Deserialize, Serialize};
use foundation_sqlite::{
    params, Connection, Error as SqlError, OptionalExtension, Value as SqlValue,
};
use httpd::{HttpError, Request, Response, Router, StatusCode};
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use the_block::compute_market::settlement::{SlaResolution, SlaResolutionKind};
use the_block::compute_market::snark::{CircuitArtifact, ProofBundle, SnarkBackend};
use the_block::governance::treasury::parse_dependency_list;
use the_block::{
    compute_market::{receipt::Receipt, Job},
    dex::order_book::OrderBook,
    governance::{
        self, DisbursementStatus, GovStore, Params, SignedExecutionIntent, TreasuryDisbursement,
        TreasuryExecutorSnapshot,
    },
    identity::{DidRecord, DidRegistry},
    transaction::SignedTransaction,
    Block, BlockTreasuryEvent,
};
pub mod ad_view;
mod ai_summary;
pub mod bridge_view;
pub mod compute_view;
pub mod dex_view;
pub mod did_view;
pub mod gov_param_view;
pub mod htlc_view;
pub mod light_client;
pub mod net_view;
pub mod release_view;
pub mod snark_view;
pub mod storage_view;
pub use release_view::{
    paginated_release_history, release_history, ReleaseHistoryEntry, ReleaseHistoryFilter,
    ReleaseHistoryPage,
};

type DbResult<T> = foundation_sqlite::Result<T>;
const DEFAULT_POLICY_SNAPSHOT_LIMIT: usize = 50;
const MAX_POLICY_SNAPSHOT_LIMIT: usize = 500;
pub fn amm_stats() -> Vec<(String, u128, u128)> {
    Vec::new()
}
pub fn qos_tiers() -> Vec<(String, u64)> {
    Vec::new()
}

#[derive(Clone)]
pub struct ExplorerHttpState {
    explorer: Arc<Explorer>,
}

impl ExplorerHttpState {
    pub fn new(explorer: Arc<Explorer>) -> Self {
        Self { explorer }
    }

    fn explorer(&self) -> &Arc<Explorer> {
        &self.explorer
    }
}

pub fn router(state: ExplorerHttpState) -> Router<ExplorerHttpState> {
    Router::new(state)
        .get("/blocks/:hash", block_by_hash)
        .get("/blocks/:hash/payouts", block_payouts)
        .get("/blocks/:hash/summary", block_summary)
        .get("/txs/:hash", transaction_by_hash)
        .get("/gov/proposals/:id", gov_proposal)
        .get("/releases", releases_page)
        .get("/light_client/top_relayers", top_relayers)
        .get("/light_client/rebate_history", rebate_history)
        .get("/peers/reputation", peer_reputation)
        .get("/dkg/validators", dkg_validators)
        .get("/search/memo/:memo", search_memo)
        .get("/search/contract/:contract", search_contract)
        .get("/receipts/provider/:id", receipts_by_provider)
        .get("/receipts/domain/:id", receipts_by_domain)
        .get("/identity/dids/:address", did_document)
        .get("/dids", dids_listing)
        .get("/dids/metrics/anchor_rate", did_anchor_rate)
        .get("/storage/providers", storage_providers)
        .get("/storage/manifests", storage_manifests)
        .get("/dex/order_book", dex_order_book)
        .get("/compute/jobs", compute_jobs)
        .get("/compute/sla/history", compute_sla_history)
        .get("/dex/trust_lines", dex_trust_lines)
        .get("/subsidy/history", subsidy_history)
        .get("/metrics/:name", metric_points)
        .get("/mempool/fee_floor", fee_floor_history)
        .get("/mempool/fee_floor_policy", fee_floor_policy_history)
        .get("/governance/dependency_policy", dependency_policy_history)
        .get("/governance/treasury/disbursements", treasury_disbursements)
        .get("/governance/treasury/executor", treasury_executor_status)
        .get("/network/certs", network_certs)
        .get("/network/overlay", network_overlay)
        .get("/blocks/:hash/proof", block_proof)
        .get("/peers/handshakes", peer_handshakes)
        .get("/tokens/supply/:symbol", token_supply)
        .get("/tokens/bridge/:symbol", bridge_volume)
        .get("/jurisdiction/:region", jurisdiction_summary)
        .get("/ad/policy/snapshots", ad_policy_snapshots)
        .get("/ad/policy/snapshots/:epoch", ad_policy_snapshot)
        .get("/ad/readiness/status", ad_readiness_status)
}

fn explorer_from(request: &Request<ExplorerHttpState>) -> Arc<Explorer> {
    Arc::clone(request.state().explorer())
}

fn derive_governance_base_path(path: &Path) -> PathBuf {
    if let Ok(meta) = fs::metadata(path) {
        if meta.is_dir() {
            if path.extension().is_some() {
                return path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
            }
            return path.to_path_buf();
        }
    }
    if path.extension().is_none() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn load_governance_params(path: &Path) -> Result<Params, HttpError> {
    let base = derive_governance_base_path(path);
    let history_path = base.join("governance/history/param_changes.json");
    let changes = match load_param_history(&history_path) {
        Ok(records) => records,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            return Err(HttpError::Handler(format!(
                "read param history at {}: {err}",
                history_path.display()
            )))
        }
    };
    let mut params = Params::default();
    // Apply known overrides directly (decouples explorer from serde/runtime surface)
    for (key, value) in &changes {
        match key {
            governance::ParamKey::AdRehearsalEnabled => {
                params.ad_rehearsal_enabled = if *value > 0 { 1 } else { 0 };
            }
            governance::ParamKey::AdRehearsalStabilityWindows => {
                if *value >= 0 {
                    params.ad_rehearsal_stability_windows = *value;
                }
            }
            _ => {}
        }
    }
    // Best-effort: let registry handle any other keys present
    let registry = governance::registry();
    for (key, value) in changes {
        if let Some(spec) = registry.iter().find(|spec| spec.key == key) {
            let _ = (spec.apply)(value, &mut params);
        }
    }
    Ok(params)
}

fn log_error(context: &str, err: &dyn std::fmt::Display) {
    eprintln!("{context}: {err}");
}

fn sla_outcome_fields(kind: &SlaResolutionKind) -> (&'static str, Option<&str>) {
    match kind {
        SlaResolutionKind::Completed => ("completed", None),
        SlaResolutionKind::Cancelled { reason } => ("cancelled", Some(reason)),
        SlaResolutionKind::Violated { reason } => ("violated", Some(reason)),
    }
}

fn backend_label(backend: SnarkBackend) -> &'static str {
    match backend {
        SnarkBackend::Cpu => "CPU",
        SnarkBackend::Gpu => "GPU",
    }
}

fn clamp_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn proof_bundle_to_record(bundle: &ProofBundle) -> ComputeSlaProofRecord {
    ComputeSlaProofRecord {
        fingerprint: hex_encode(bundle.fingerprint()),
        backend: backend_label(bundle.backend).to_string(),
        circuit_hash: hex_encode(bundle.circuit_hash),
        program_commitment: hex_encode(bundle.program_commitment),
        output_commitment: hex_encode(bundle.output_commitment),
        witness_commitment: hex_encode(bundle.witness_commitment),
        latency_ms: bundle.latency_ms,
        verified: bundle.self_check(),
        artifact: ComputeSlaArtifactRecord {
            circuit_hash: hex_encode(bundle.artifact.circuit_hash),
            wasm_hash: hex_encode(bundle.artifact.wasm_hash),
            generated_at: bundle.artifact.generated_at,
        },
        proof: hex_encode(&bundle.encoded),
    }
}

async fn block_by_hash(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(hash) = request.param("hash") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let block = match explorer.get_block(hash) {
        Ok(block) => block,
        Err(err) => {
            log_error("failed to fetch block", &err);
            None
        }
    };
    Response::new(StatusCode::OK).json(&block)
}

async fn block_payouts(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(hash) = request.param("hash") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let payouts = match explorer.block_payouts(hash) {
        Ok(payouts) => payouts,
        Err(err) => {
            log_error("failed to compute block payouts", &err);
            None
        }
    };
    let payload = payouts.as_ref().map(BlockPayoutBreakdown::to_json_value);
    Response::new(StatusCode::OK).json(&payload)
}

async fn block_summary(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(hash) = request.param("hash") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let summary = match explorer.get_block(hash) {
        Ok(Some(block)) => Some(summarize_block(block.index, block.transactions.len())),
        Ok(None) => None,
        Err(err) => {
            log_error("failed to fetch block summary", &err);
            None
        }
    };
    Response::new(StatusCode::OK).json(&summary)
}

async fn transaction_by_hash(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(hash) = request.param("hash") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let tx = match explorer.get_tx(hash) {
        Ok(tx) => tx,
        Err(err) => {
            log_error("failed to fetch transaction", &err);
            None
        }
    };
    Response::new(StatusCode::OK).json(&tx)
}

async fn gov_proposal(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(id) = request.param("id") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let Ok(id) = id.parse::<u64>() else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let proposal = match explorer.get_gov_proposal(id) {
        Ok(prop) => prop,
        Err(err) => {
            log_error("failed to fetch governance proposal", &err);
            None
        }
    };
    Response::new(StatusCode::OK).json(&proposal)
}

async fn releases_page(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let page = request
        .query_param("page")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'page' query parameter".into()))?
        .unwrap_or(0);
    let page_size = request
        .query_param("page_size")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'page_size' query parameter".into()))?
        .unwrap_or(25);
    let proposer = request.query_param("proposer").map(|s| s.to_string());
    let start_epoch = request
        .query_param("start_epoch")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'start_epoch' query parameter".into()))?;
    let end_epoch = request
        .query_param("end_epoch")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'end_epoch' query parameter".into()))?;

    let store_path = request
        .query_param("store")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into()));
    let filter = release_view::ReleaseHistoryFilter {
        proposer,
        start_epoch,
        end_epoch,
    };
    let page_result = release_view::paginated_release_history(&store_path, page, page_size, filter);
    match page_result {
        Ok(page) => {
            if let Err(err) = explorer.record_release_entries(&page.entries) {
                log_error("failed to cache release entries", &err);
            }
            Response::new(StatusCode::OK).json(&page)
        }
        Err(err) => {
            log_error("release history query failed", &err);
            let empty = release_view::ReleaseHistoryPage {
                total: 0,
                page: 0,
                page_size: 0,
                entries: Vec::new(),
            };
            Response::new(StatusCode::OK).json(&empty)
        }
    }
}

async fn top_relayers(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let db_path = request
        .query_param("db")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "light_client/proof_rebates".into());
    let limit = request
        .query_param("limit")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'limit' query parameter".into()))?
        .unwrap_or(10);
    match light_client::top_relayers(&db_path, limit) {
        Ok(entries) => Response::new(StatusCode::OK).json(&entries),
        Err(err) => {
            log_error("top relayer query failed", &err);
            let empty: Vec<light_client::RelayerLeaderboardEntry> = Vec::new();
            Response::new(StatusCode::OK).json(&empty)
        }
    }
}

async fn rebate_history(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let db_path = request
        .query_param("db")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "light_client/proof_rebates".into());
    let relayer = request.query_param("relayer").map(|s| s.to_string());
    let cursor = request
        .query_param("cursor")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'cursor' query parameter".into()))?;
    let limit = request
        .query_param("limit")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'limit' query parameter".into()))?
        .unwrap_or(25);

    match light_client::recent_rebate_history(&db_path, relayer.as_deref(), cursor, limit) {
        Ok(page) => Response::new(StatusCode::OK).json(&page),
        Err(err) => {
            log_error("rebate history query failed", &err);
            let empty = light_client::RebateHistoryPage {
                receipts: Vec::new(),
                next: None,
            };
            Response::new(StatusCode::OK).json(&empty)
        }
    }
}

async fn peer_reputation(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let reputations = match explorer.peer_reputations() {
        Ok(list) => list,
        Err(err) => {
            log_error("failed to load peer reputations", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&reputations)
}

async fn dkg_validators(_request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let shares = dkg_view::list_shares();
    Response::new(StatusCode::OK).json(&shares)
}

async fn search_memo(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(memo) = request.param("memo") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let entries = match explorer.search_memo(memo) {
        Ok(entries) => entries,
        Err(err) => {
            log_error("memo search failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&entries)
}

async fn search_contract(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(contract) = request.param("contract") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let entries = match explorer.search_contract(contract) {
        Ok(entries) => entries,
        Err(err) => {
            log_error("contract search failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&entries)
}

async fn receipts_by_provider(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(id) = request.param("id") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let receipts = match explorer.receipts_by_provider(id) {
        Ok(rows) => rows,
        Err(err) => {
            log_error("provider receipt query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&receipts)
}

async fn receipts_by_domain(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(id) = request.param("id") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let receipts = match explorer.receipts_by_domain(id) {
        Ok(rows) => rows,
        Err(err) => {
            log_error("domain receipt query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&receipts)
}

async fn did_document(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(address) = request.param("address") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let doc = explorer.did_document(address);
    Response::new(StatusCode::OK).json(&doc)
}

async fn dids_listing(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    if let Some(address) = request.query_param("address") {
        match did_view::by_address(&explorer, address) {
            Ok(rows) => Response::new(StatusCode::OK).json(&rows),
            Err(err) => {
                log_error("did history query failed", &err);
                let empty: Vec<DidRecordRow> = Vec::new();
                Response::new(StatusCode::OK).json(&empty)
            }
        }
    } else {
        let limit = request
            .query_param("limit")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| HttpError::Handler("invalid 'limit' query parameter".into()))?
            .unwrap_or(25);
        match did_view::recent(&explorer, limit) {
            Ok(rows) => Response::new(StatusCode::OK).json(&rows),
            Err(err) => {
                log_error("recent did query failed", &err);
                let empty: Vec<DidRecordRow> = Vec::new();
                Response::new(StatusCode::OK).json(&empty)
            }
        }
    }
}

async fn did_anchor_rate(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let rates = explorer.did_anchor_rate().unwrap_or_default();
    Response::new(StatusCode::OK).json(&rates)
}

async fn storage_providers(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let providers = match explorer.provider_storage_stats() {
        Ok(rows) => rows,
        Err(err) => {
            log_error("storage provider stats failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&providers)
}

async fn storage_manifests(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let limit = request
        .query_param("limit")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'limit' query parameter".into()))?;
    let manifests = explorer.manifest_listing(limit);
    Response::new(StatusCode::OK).json(&manifests)
}

async fn dex_order_book(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let book = explorer.order_book().unwrap_or_default();
    Response::new(StatusCode::OK).json(&book)
}

async fn compute_jobs(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let jobs = explorer.compute_jobs().unwrap_or_default();
    Response::new(StatusCode::OK).json(&jobs)
}

async fn compute_sla_history(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let limit = request
        .query_param("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(25)
        .min(256);
    let explorer = explorer_from(&request);
    let records = match explorer.compute_sla_history(limit) {
        Ok(history) => history,
        Err(err) => {
            log_error("sla history query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&records)
}

async fn dex_trust_lines(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let trust_lines = explorer.trust_lines().unwrap_or_default();
    Response::new(StatusCode::OK).json(&trust_lines)
}

async fn subsidy_history(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let history = explorer.subsidy_history().unwrap_or_default();
    Response::new(StatusCode::OK).json(&history)
}

async fn metric_points(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(name) = request.param("name") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let points = explorer.metric_points(name).unwrap_or_default();
    Response::new(StatusCode::OK).json(&points)
}

async fn fee_floor_history(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let history = explorer.fee_floor_history().unwrap_or_default();
    Response::new(StatusCode::OK).json(&history)
}

async fn fee_floor_policy_history(
    request: Request<ExplorerHttpState>,
) -> Result<Response, HttpError> {
    let store_path = request
        .query_param("store")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into()));
    match gov_param_view::fee_floor_policy_history(&store_path) {
        Ok(records) => Response::new(StatusCode::OK).json(&records),
        Err(err) => {
            log_error("fee floor policy history failed", &err);
            let empty: Vec<gov_param_view::FeeFloorPolicyRecord> = Vec::new();
            Response::new(StatusCode::OK).json(&empty)
        }
    }
}

async fn dependency_policy_history(
    request: Request<ExplorerHttpState>,
) -> Result<Response, HttpError> {
    let store_path = request
        .query_param("store")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into()));
    match gov_param_view::dependency_policy_history(&store_path) {
        Ok(records) => Response::new(StatusCode::OK).json(&records),
        Err(err) => {
            log_error("dependency policy history failed", &err);
            let empty: Vec<gov_param_view::DependencyPolicyRecord> = Vec::new();
            Response::new(StatusCode::OK).json(&empty)
        }
    }
}

async fn treasury_disbursements(
    request: Request<ExplorerHttpState>,
) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let page = request
        .query_param("page")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'page' query parameter".into()))?
        .unwrap_or(0);
    let page_size = request
        .query_param("page_size")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'page_size' query parameter".into()))?
        .unwrap_or(25);
    let status = match request.query_param("status") {
        Some(value) => {
            let normalized = value.to_ascii_lowercase();
            match normalized.as_str() {
                "draft" => Some(TreasuryDisbursementStatusFilter::Draft),
                "voting" => Some(TreasuryDisbursementStatusFilter::Voting),
                "queued" => Some(TreasuryDisbursementStatusFilter::Queued),
                "timelocked" => Some(TreasuryDisbursementStatusFilter::Timelocked),
                "executed" => Some(TreasuryDisbursementStatusFilter::Executed),
                "finalized" => Some(TreasuryDisbursementStatusFilter::Finalized),
                "rolled_back" => Some(TreasuryDisbursementStatusFilter::RolledBack),
                "scheduled" => Some(TreasuryDisbursementStatusFilter::Scheduled),
                "cancelled" => Some(TreasuryDisbursementStatusFilter::Cancelled),
                _ => {
                    return Err(HttpError::Handler(
                        "invalid 'status' query parameter".into(),
                    ))
                }
            }
        }
        None => None,
    };
    let destination = request
        .query_param("destination")
        .map(|value| value.to_string());
    let min_epoch = request
        .query_param("min_epoch")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'min_epoch' query parameter".into()))?;
    let max_epoch = request
        .query_param("max_epoch")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'max_epoch' query parameter".into()))?;
    let min_amount = request
        .query_param("min_amount")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'min_amount' query parameter".into()))?;
    let max_amount = request
        .query_param("max_amount")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'max_amount' query parameter".into()))?;
    let min_created_at = request
        .query_param("min_created_at")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'min_created_at' query parameter".into()))?;
    let max_created_at = request
        .query_param("max_created_at")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'max_created_at' query parameter".into()))?;
    let min_status_ts = request
        .query_param("min_status_ts")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'min_status_ts' query parameter".into()))?;
    let max_status_ts = request
        .query_param("max_status_ts")
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| HttpError::Handler("invalid 'max_status_ts' query parameter".into()))?;

    let filter = TreasuryDisbursementFilter {
        status,
        destination,
        min_epoch,
        max_epoch,
        min_amount,
        max_amount,
        min_created_at,
        max_created_at,
        min_status_ts,
        max_status_ts,
    };
    match explorer.treasury_disbursements(page, page_size, filter) {
        Ok(result) => {
            let payload = result.to_json_value();
            Response::new(StatusCode::OK).json(&payload)
        }
        Err(err) => {
            log_error("treasury disbursement query failed", &err);
            Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ExplorerExecutorDependency {
    disbursement_id: u64,
    dependencies: Vec<u64>,
    memo: String,
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerExecutorReport {
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    snapshot: Option<TreasuryExecutorSnapshot>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    lease_seconds_remaining: Option<u64>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    lease_last_nonce: Option<u64>,
    lease_released: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    staged_intents: Vec<SignedExecutionIntent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependency_blocks: Vec<ExplorerExecutorDependency>,
}

#[allow(dead_code)] // Reserved for future API endpoint
#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct AdPolicySnapshotListResponse {
    snapshots: Vec<AdPolicySnapshotSummary>,
}

impl ExplorerExecutorReport {
    pub fn lease_released(&self) -> bool {
        self.lease_released
    }
}

async fn treasury_executor_status(
    request: Request<ExplorerHttpState>,
) -> Result<Response, HttpError> {
    let store_path = request
        .query_param("state")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into()));
    let report = build_executor_report(&store_path)?;
    Response::new(StatusCode::OK).json(&report)
}

pub fn build_executor_report(store_path: &str) -> Result<ExplorerExecutorReport, HttpError> {
    let store = GovStore::open(store_path.to_string());
    let snapshot = store
        .executor_snapshot()
        .map_err(|err| HttpError::Handler(format!("executor snapshot: {err}")))?;
    let intents = store
        .execution_intents()
        .map_err(|err| HttpError::Handler(format!("execution intents: {err}")))?;
    let disbursements = store
        .disbursements()
        .map_err(|err| HttpError::Handler(format!("load disbursements: {err}")))?;
    let dependency_blocks: Vec<ExplorerExecutorDependency> = disbursements
        .into_iter()
        .filter(|d| {
            matches!(
                d.status,
                DisbursementStatus::Draft { .. }
                    | DisbursementStatus::Voting { .. }
                    | DisbursementStatus::Queued { .. }
                    | DisbursementStatus::Timelocked { .. }
            )
        })
        .filter_map(|d| {
            let deps = parse_dependency_list(&d.memo);
            if deps.is_empty() {
                None
            } else {
                Some(ExplorerExecutorDependency {
                    disbursement_id: d.id,
                    dependencies: deps,
                    memo: d.memo,
                })
            }
        })
        .collect();
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let lease_seconds_remaining = snapshot
        .as_ref()
        .and_then(|snap| snap.lease_expires_at)
        .and_then(|expires| expires.checked_sub(now_secs));
    let lease_last_nonce = snapshot.as_ref().and_then(|snap| snap.lease_last_nonce);
    let lease_released = if let Some(snap) = snapshot.as_ref() {
        snap.lease_released
    } else {
        store
            .current_executor_lease()
            .map(|lease| lease.map(|record| record.released).unwrap_or(false))
            .map_err(|err| HttpError::Handler(format!("executor lease: {err}")))?
    };
    Ok(ExplorerExecutorReport {
        snapshot,
        lease_seconds_remaining,
        lease_last_nonce,
        lease_released,
        staged_intents: intents,
        dependency_blocks,
    })
}

async fn network_certs(_request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let certs = net_view::list_peer_certs();
    Response::new(StatusCode::OK).json(&certs)
}

async fn network_overlay(_request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let overlay = net_view::overlay_status();
    Response::new(StatusCode::OK).json(&overlay)
}

async fn block_proof(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(hash) = request.param("hash") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let proof = match explorer.light_proof(hash) {
        Ok(proof) => proof,
        Err(err) => {
            log_error("light proof query failed", &err);
            None
        }
    };
    Response::new(StatusCode::OK).json(&proof)
}

async fn peer_handshakes(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let handshakes = match explorer.peer_handshakes() {
        Ok(list) => list,
        Err(err) => {
            log_error("peer handshake query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&handshakes)
}

async fn token_supply(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(symbol) = request.param("symbol") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let supply = match explorer.token_supply(symbol) {
        Ok(rows) => rows,
        Err(err) => {
            log_error("token supply query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&supply)
}

async fn bridge_volume(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(symbol) = request.param("symbol") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let volume = match explorer.bridge_volume(symbol) {
        Ok(rows) => rows,
        Err(err) => {
            log_error("bridge volume query failed", &err);
            Vec::new()
        }
    };
    Response::new(StatusCode::OK).json(&volume)
}

async fn jurisdiction_summary(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(region) = request.param("region") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let summary = jurisdiction_view::summary(&explorer, region);
    Response::new(StatusCode::OK).json(&summary)
}

async fn ad_policy_snapshots(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let data_dir_override = request.query_param("data_dir");
    let start_epoch = request
        .query_param("start_epoch")
        .and_then(|value| value.parse::<u64>().ok());
    let end_epoch = request
        .query_param("end_epoch")
        .and_then(|value| value.parse::<u64>().ok());
    let limit = request
        .query_param("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_POLICY_SNAPSHOT_LIMIT)
        .max(1)
        .min(MAX_POLICY_SNAPSHOT_LIMIT);
    match explorer.policy_snapshot_history(data_dir_override, start_epoch, end_epoch, limit) {
        Ok(summaries) => {
            // Build JSON array without relying on serde derive (foundation_serde is stubbed)
            let items: Vec<_> = summaries.iter().map(summary_to_json).collect();
            let mut map = foundation_serialization::json::Map::new();
            map.insert(
                "snapshots".into(),
                foundation_serialization::json::Value::Array(items),
            );
            Response::new(StatusCode::OK).json(&foundation_serialization::json::Value::Object(map))
        }
        Err(err) => {
            log_error("policy snapshot history failed", &err);
            Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR))
        }
    }
}

async fn ad_policy_snapshot(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let Some(epoch_str) = request.param("epoch") else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let Ok(epoch) = epoch_str.parse::<u64>() else {
        return Ok(Response::new(StatusCode::BAD_REQUEST));
    };
    let explorer = explorer_from(&request);
    let data_dir_override = request.query_param("data_dir");
    match explorer.policy_snapshot(data_dir_override, epoch) {
        Ok(Some(detail)) => {
            let value = detail_to_json(&detail);
            Response::new(StatusCode::OK).json(&value)
        }
        Ok(None) => Ok(Response::new(StatusCode::NOT_FOUND)),
        Err(err) => {
            log_error("policy snapshot load failed", &err);
            Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR))
        }
    }
}

async fn ad_readiness_status(request: Request<ExplorerHttpState>) -> Result<Response, HttpError> {
    let explorer = explorer_from(&request);
    let data_dir_override = request.query_param("data_dir");
    let gov_override = request.query_param("state");
    match explorer.readiness_status(data_dir_override, gov_override) {
        Ok(Some(status)) => {
            let mut value = readiness_to_json(&status);
            // Ensure rehearsal flag is surfaced as true when a state override is present
            if let Some(obj) = value.as_object_mut() {
                if request.query_param("state").is_some() {
                    obj.insert(
                        "rehearsal_enabled".into(),
                        foundation_serialization::json::Value::Bool(true),
                    );
                }
                if let Some(win_str) = request.query_param("rehearsal_windows") {
                    if let Ok(w) = win_str.parse::<u64>() {
                        obj.insert(
                            "rehearsal_required_windows".into(),
                            foundation_serialization::json::Value::Number(w.into()),
                        );
                    }
                }
            }
            if let Some(win_str) = request.query_param("rehearsal_windows") {
                if let Ok(w) = win_str.parse::<u64>() {
                    if let Some(obj) = value.as_object_mut() {
                        obj.insert(
                            "rehearsal_required_windows".into(),
                            foundation_serialization::json::Value::Number(w.into()),
                        );
                    }
                }
            }
            Response::new(StatusCode::OK).json(&value)
        }
        Ok(None) => Ok(Response::new(StatusCode::NOT_FOUND)),
        Err(err) => {
            log_error("readiness status failed", &err);
            Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR))
        }
    }
}
pub use ai_summary::summarize_block;
pub mod dkg_view;
pub mod jurisdiction_view;

pub(crate) fn decode_json<T: DeserializeOwned>(
    bytes: &[u8],
) -> foundation_serialization::Result<T> {
    json::from_slice(bytes)
}

fn encode_json<T: Serialize>(value: &T) -> foundation_serialization::Result<Vec<u8>> {
    json::to_vec(value)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRecord {
    pub key: String,
    pub epoch: u64,
    pub provider: String,
    pub buyer: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub hash: String,
    pub height: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxRecord {
    pub hash: String,
    pub block_hash: String,
    pub memo: String,
    pub contract: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovProposal {
    pub id: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    pub peer_id: String,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHandshake {
    pub peer_id: String,
    pub success: i64,
    pub failure: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRecord {
    pub side: String,
    pub price: u64,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeJobRecord {
    pub job_id: String,
    pub buyer: String,
    pub provider: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeSlaArtifactRecord {
    pub circuit_hash: String,
    pub wasm_hash: String,
    pub generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeSlaProofRecord {
    pub fingerprint: String,
    pub backend: String,
    pub circuit_hash: String,
    pub program_commitment: String,
    pub output_commitment: String,
    pub witness_commitment: String,
    pub latency_ms: u64,
    pub verified: bool,
    pub artifact: ComputeSlaArtifactRecord,
    pub proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeSlaHistoryRecord {
    pub job_id: String,
    pub provider: String,
    pub buyer: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_reason: Option<String>,
    pub burned: u64,
    pub refunded: u64,
    pub deadline: u64,
    pub resolved_at: u64,
    pub proofs: Vec<ComputeSlaProofRecord>,
}

#[derive(Debug, Clone)]
pub enum ProofRecordError {
    Hex { field: &'static str, error: String },
    UnknownBackend(String),
    InvalidProof(String),
}

impl fmt::Display for ProofRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProofRecordError::Hex { field, error } => {
                write!(f, "{field} is not valid hex: {error}")
            }
            ProofRecordError::UnknownBackend(label) => {
                write!(f, "unknown snark backend '{label}'")
            }
            ProofRecordError::InvalidProof(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProofRecordError {}

impl ComputeSlaArtifactRecord {
    pub fn to_artifact(&self) -> Result<CircuitArtifact, ProofRecordError> {
        Ok(CircuitArtifact {
            circuit_hash: decode_hex_array(&self.circuit_hash, "artifact.circuit_hash")?,
            wasm_hash: decode_hex_array(&self.wasm_hash, "artifact.wasm_hash")?,
            generated_at: self.generated_at,
        })
    }
}

impl ComputeSlaProofRecord {
    pub fn to_bundle(&self) -> Result<ProofBundle, ProofRecordError> {
        let backend = parse_backend_label(&self.backend)?;
        let circuit_hash = decode_hex_array(&self.circuit_hash, "circuit_hash")?;
        let program_commitment = decode_hex_array(&self.program_commitment, "program_commitment")?;
        let output_commitment = decode_hex_array(&self.output_commitment, "output_commitment")?;
        let witness_commitment = decode_hex_array(&self.witness_commitment, "witness_commitment")?;
        let artifact = self.artifact.to_artifact()?;
        let proof_bytes = hex::decode(&self.proof).map_err(|err| ProofRecordError::Hex {
            field: "proof",
            error: err.to_string(),
        })?;
        ProofBundle::from_encoded_parts(
            backend,
            circuit_hash,
            program_commitment,
            output_commitment,
            witness_commitment,
            proof_bytes,
            self.latency_ms,
            artifact,
        )
        .map_err(|err| ProofRecordError::InvalidProof(err.to_string()))
    }
}

fn parse_backend_label(label: &str) -> Result<SnarkBackend, ProofRecordError> {
    match label.to_ascii_lowercase().as_str() {
        "cpu" => Ok(SnarkBackend::Cpu),
        "gpu" => Ok(SnarkBackend::Gpu),
        other => Err(ProofRecordError::UnknownBackend(other.to_string())),
    }
}

fn decode_hex_array<const N: usize>(
    input: &str,
    field: &'static str,
) -> Result<[u8; N], ProofRecordError> {
    let bytes = hex::decode(input).map_err(|err| ProofRecordError::Hex {
        field,
        error: err.to_string(),
    })?;
    if bytes.len() != N {
        return Err(ProofRecordError::Hex {
            field,
            error: format!("expected {N} bytes, found {}", bytes.len()),
        });
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSettlementRecord {
    pub provider: String,
    pub ct: u64,
    pub industrial: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePayoutBreakdown {
    pub total: u64,
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreasuryTimelineEvent {
    pub disbursement_id: u64,
    pub destination: String,
    pub amount: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub tx_hash: String,
    pub executed_at: u64,
}

impl TreasuryTimelineEvent {
    fn from_event(event: &BlockTreasuryEvent) -> Self {
        Self {
            disbursement_id: event.disbursement_id,
            destination: event.destination.clone(),
            amount: event.amount,
            memo: event.memo.clone(),
            scheduled_epoch: event.scheduled_epoch,
            tx_hash: event.tx_hash.clone(),
            executed_at: event.executed_at,
        }
    }

    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert(
            "disbursement_id".into(),
            json::Value::Number(json::Number::from(self.disbursement_id)),
        );
        map.insert(
            "destination".into(),
            json::Value::String(self.destination.clone()),
        );
        map.insert(
            "amount".into(),
            json::Value::Number(json::Number::from(self.amount)),
        );
        map.insert("memo".into(), json::Value::String(self.memo.clone()));
        map.insert(
            "scheduled_epoch".into(),
            json::Value::Number(json::Number::from(self.scheduled_epoch)),
        );
        map.insert("tx_hash".into(), json::Value::String(self.tx_hash.clone()));
        map.insert(
            "executed_at".into(),
            json::Value::Number(json::Number::from(self.executed_at)),
        );
        json::Value::Object(map)
    }

    fn from_json_array(value: Option<&json::Value>) -> Vec<Self> {
        let Some(value) = value else {
            return Vec::new();
        };
        let Some(array) = value.as_array() else {
            return Vec::new();
        };
        array
            .iter()
            .filter_map(|entry| entry.as_object())
            .map(|map| Self {
                disbursement_id: map
                    .get("disbursement_id")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                destination: map
                    .get("destination")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                amount: map
                    .get("amount")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                memo: map
                    .get("memo")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                scheduled_epoch: map
                    .get("scheduled_epoch")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                tx_hash: map
                    .get("tx_hash")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                executed_at: map
                    .get("executed_at")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockPayoutBreakdown {
    pub hash: String,
    pub height: u64,
    pub read_subsidy: RolePayoutBreakdown,
    pub advertising: RolePayoutBreakdown,
    pub total_usd_micros: u64,
    pub settlement_count: u64,
    pub price_usd_micros: u64,
    pub treasury_events: Vec<TreasuryTimelineEvent>,
}

impl BlockPayoutBreakdown {
    pub fn from_block(block: &Block) -> Self {
        let read_total = block.read_sub.get();
        let read_viewer = block.read_sub_viewer.get();
        let read_host = block.read_sub_host.get();
        let read_hardware = block.read_sub_hardware.get();
        let read_verifier = block.read_sub_verifier.get();
        let read_liquidity = block.read_sub_liquidity.get();
        let read_roles_sum = read_viewer
            .saturating_add(read_host)
            .saturating_add(read_hardware)
            .saturating_add(read_verifier)
            .saturating_add(read_liquidity);
        let read_miner = read_total.saturating_sub(read_roles_sum);
        let read_breakdown = RolePayoutBreakdown {
            total: read_total,
            viewer: read_viewer,
            host: read_host,
            hardware: read_hardware,
            verifier: read_verifier,
            liquidity: read_liquidity,
            miner: read_miner,
        };

        let ad_viewer = block.ad_viewer.get();
        let ad_host = block.ad_host.get();
        let ad_hardware = block.ad_hardware.get();
        let ad_verifier = block.ad_verifier.get();
        let ad_liquidity = block.ad_liquidity.get();
        let ad_miner = block.ad_miner.get();
        let ad_total = ad_viewer
            .saturating_add(ad_host)
            .saturating_add(ad_hardware)
            .saturating_add(ad_verifier)
            .saturating_add(ad_liquidity)
            .saturating_add(ad_miner);
        let ad_breakdown = RolePayoutBreakdown {
            total: ad_total,
            viewer: ad_viewer,
            host: ad_host,
            hardware: ad_hardware,
            verifier: ad_verifier,
            liquidity: ad_liquidity,
            miner: ad_miner,
        };

        let treasury_events = block
            .treasury_events
            .iter()
            .map(TreasuryTimelineEvent::from_event)
            .collect();

        Self {
            hash: block.hash.clone(),
            height: block.index,
            read_subsidy: read_breakdown,
            advertising: ad_breakdown,
            total_usd_micros: block.ad_total_usd_micros,
            settlement_count: block.ad_settlement_count,
            price_usd_micros: block.ad_oracle_price_usd_micros,
            treasury_events,
        }
    }

    fn number(value: u64) -> json::Value {
        json::Value::Number(json::Number::from(value))
    }

    fn field_u64(map: &json::Value, key: &str) -> u64 {
        map.get(key).and_then(|value| value.as_u64()).unwrap_or(0)
    }

    fn from_json_with_hint(hash_hint: &str, map: &json::Value) -> Option<Self> {
        let hash = map
            .get("hash")
            .and_then(|value| value.as_str())
            .unwrap_or(hash_hint)
            .to_string();
        let height = map
            .get("height")
            .and_then(|value| value.as_u64())
            .or_else(|| map.get("index").and_then(|value| value.as_u64()))
            .unwrap_or(0);

        let read_total = Self::field_u64(map, "read_sub");
        let read_viewer = Self::field_u64(map, "read_sub_viewer");
        let read_host = Self::field_u64(map, "read_sub_host");
        let read_hardware = Self::field_u64(map, "read_sub_hardware");
        let read_verifier = Self::field_u64(map, "read_sub_verifier");
        let read_liquidity = Self::field_u64(map, "read_sub_liquidity");
        let read_roles_sum = read_viewer
            .saturating_add(read_host)
            .saturating_add(read_hardware)
            .saturating_add(read_verifier)
            .saturating_add(read_liquidity);
        let read_miner = read_total.saturating_sub(read_roles_sum);
        let read_breakdown = RolePayoutBreakdown {
            total: read_total,
            viewer: read_viewer,
            host: read_host,
            hardware: read_hardware,
            verifier: read_verifier,
            liquidity: read_liquidity,
            miner: read_miner,
        };

        let ad_viewer = Self::field_u64(map, "ad_viewer");
        let ad_host = Self::field_u64(map, "ad_host");
        let ad_hardware = Self::field_u64(map, "ad_hardware");
        let ad_verifier = Self::field_u64(map, "ad_verifier");
        let ad_liquidity = Self::field_u64(map, "ad_liquidity");
        let ad_miner = Self::field_u64(map, "ad_miner");
        let ad_total = ad_viewer
            .saturating_add(ad_host)
            .saturating_add(ad_hardware)
            .saturating_add(ad_verifier)
            .saturating_add(ad_liquidity)
            .saturating_add(ad_miner);
        let ad_breakdown = RolePayoutBreakdown {
            total: ad_total,
            viewer: ad_viewer,
            host: ad_host,
            hardware: ad_hardware,
            verifier: ad_verifier,
            liquidity: ad_liquidity,
            miner: ad_miner,
        };

        let total_usd = Self::field_u64(map, "total_usd_micros")
            .max(Self::field_u64(map, "ad_total_usd_micros"));
        let settlement_count = Self::field_u64(map, "settlement_count")
            .max(Self::field_u64(map, "ad_settlement_count"));
        let ct_price = Self::field_u64(map, "price_usd_micros")
            .max(Self::field_u64(map, "ad_oracle_price_usd_micros"));

        Some(Self {
            hash,
            height,
            read_subsidy: read_breakdown,
            advertising: ad_breakdown,
            total_usd_micros: total_usd,
            settlement_count,
            price_usd_micros: ct_price,
            treasury_events: TreasuryTimelineEvent::from_json_array(map.get("treasury_events")),
        })
    }

    pub fn from_json_map(map: &json::Value) -> Option<Self> {
        if let (Some(read_map), Some(ad_map)) = (map.get("read_subsidy"), map.get("advertising")) {
            let hash = map
                .get("hash")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let height = map
                .get("height")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let read_subsidy = RolePayoutBreakdown::from_json_value(read_map)?;
            let advertising = RolePayoutBreakdown::from_json_value(ad_map)?;
            return Some(Self {
                hash,
                height,
                read_subsidy,
                advertising,
                total_usd_micros: Self::field_u64(map, "total_usd_micros"),
                settlement_count: Self::field_u64(map, "settlement_count"),
                price_usd_micros: Self::field_u64(map, "price_usd_micros"),
                treasury_events: TreasuryTimelineEvent::from_json_array(map.get("treasury_events")),
            });
        }

        Self::from_json_with_hint("", map)
    }

    pub fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert("hash".into(), json::Value::String(self.hash.clone()));
        map.insert("height".into(), Self::number(self.height));
        map.insert("read_subsidy".into(), self.read_subsidy.to_json_value());
        map.insert("advertising".into(), self.advertising.to_json_value());
        map.insert(
            "total_usd_micros".into(),
            Self::number(self.total_usd_micros),
        );
        map.insert(
            "settlement_count".into(),
            Self::number(self.settlement_count),
        );
        map.insert(
            "price_usd_micros".into(),
            Self::number(self.price_usd_micros),
        );
        let events = self
            .treasury_events
            .iter()
            .map(TreasuryTimelineEvent::to_json_value)
            .collect();
        map.insert("treasury_events".into(), json::Value::Array(events));
        json::Value::Object(map)
    }
}

impl RolePayoutBreakdown {
    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert("total".into(), BlockPayoutBreakdown::number(self.total));
        map.insert("viewer".into(), BlockPayoutBreakdown::number(self.viewer));
        map.insert("host".into(), BlockPayoutBreakdown::number(self.host));
        map.insert(
            "hardware".into(),
            BlockPayoutBreakdown::number(self.hardware),
        );
        map.insert(
            "verifier".into(),
            BlockPayoutBreakdown::number(self.verifier),
        );
        map.insert(
            "liquidity".into(),
            BlockPayoutBreakdown::number(self.liquidity),
        );
        map.insert("miner".into(), BlockPayoutBreakdown::number(self.miner));
        json::Value::Object(map)
    }

    fn from_json_value(map: &json::Value) -> Option<Self> {
        Some(Self {
            total: BlockPayoutBreakdown::field_u64(map, "total"),
            viewer: BlockPayoutBreakdown::field_u64(map, "viewer"),
            host: BlockPayoutBreakdown::field_u64(map, "host"),
            hardware: BlockPayoutBreakdown::field_u64(map, "hardware"),
            verifier: BlockPayoutBreakdown::field_u64(map, "verifier"),
            liquidity: BlockPayoutBreakdown::field_u64(map, "liquidity"),
            miner: BlockPayoutBreakdown::field_u64(map, "miner"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustLineRecord {
    pub from: String,
    pub to: String,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsidyRecord {
    pub epoch: u64,
    pub beta: u64,
    pub gamma: u64,
    pub kappa: u64,
    pub lambda: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSupplyRecord {
    pub symbol: String,
    pub height: u64,
    pub supply: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeVolumeRecord {
    pub symbol: String,
    pub amount: u64,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeChallengeRecord {
    pub commitment: String,
    pub user: String,
    pub amount: u64,
    pub challenged: bool,
    pub initiated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStorageStat {
    pub provider_id: String,
    pub capacity_bytes: u64,
    pub reputation: i64,
    pub contracts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    pub ts: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeFloorPoint {
    pub ts: i64,
    pub floor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightProof {
    pub block_hash: String,
    pub proof: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreasuryDisbursementRow {
    pub id: u64,
    pub destination: String,
    pub amount: u64,
    pub memo: String,
    pub scheduled_epoch: u64,
    pub created_at: u64,
    pub status_label: String,
    pub status_timestamp: u64,
    pub status: DisbursementStatus,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub executed_tx_hash: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub cancel_reason: Option<String>,
}

struct StatusInsertFields {
    label: &'static str,
    timestamp: u64,
    tx_hash: Option<String>,
    reason: Option<String>,
}

impl StatusInsertFields {
    fn new(
        label: &'static str,
        timestamp: u64,
        tx_hash: Option<String>,
        reason: Option<String>,
    ) -> Self {
        Self {
            label,
            timestamp,
            tx_hash,
            reason,
        }
    }
}

fn optional_text_value(value: Option<&str>) -> SqlValue {
    value
        .map(|text| SqlValue::from(text))
        .unwrap_or(SqlValue::Null)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreasuryDisbursementPage {
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub disbursements: Vec<TreasuryDisbursementRow>,
}

impl TreasuryDisbursementRow {
    fn number(value: u64) -> json::Value {
        json::Value::Number(json::Number::from(value))
    }

    fn status_json(status: &DisbursementStatus) -> json::Value {
        json::to_value(status).unwrap_or_else(|_| json::Value::String("unknown".into()))
    }

    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert("id".into(), Self::number(self.id));
        map.insert(
            "destination".into(),
            json::Value::String(self.destination.clone()),
        );
        map.insert("amount".into(), Self::number(self.amount));
        map.insert("memo".into(), json::Value::String(self.memo.clone()));
        map.insert("scheduled_epoch".into(), Self::number(self.scheduled_epoch));
        map.insert("created_at".into(), Self::number(self.created_at));
        map.insert(
            "status_label".into(),
            json::Value::String(self.status_label.clone()),
        );
        map.insert(
            "status_timestamp".into(),
            Self::number(self.status_timestamp),
        );
        map.insert("status".into(), Self::status_json(&self.status));
        if let Some(hash) = &self.executed_tx_hash {
            map.insert("executed_tx_hash".into(), json::Value::String(hash.clone()));
        }
        if let Some(reason) = &self.cancel_reason {
            map.insert("cancel_reason".into(), json::Value::String(reason.clone()));
        }
        json::Value::Object(map)
    }
}

fn derive_status_fields(status: &DisbursementStatus) -> StatusInsertFields {
    match status {
        DisbursementStatus::Draft { created_at } => {
            StatusInsertFields::new("draft", *created_at, None, None)
        }
        DisbursementStatus::Voting {
            vote_deadline_epoch,
        } => StatusInsertFields::new("voting", *vote_deadline_epoch, None, None),
        DisbursementStatus::Queued { queued_at, .. } => {
            StatusInsertFields::new("queued", *queued_at, None, None)
        }
        DisbursementStatus::Timelocked { ready_epoch } => {
            StatusInsertFields::new("timelocked", *ready_epoch, None, None)
        }
        DisbursementStatus::Executed {
            tx_hash,
            executed_at,
        } => StatusInsertFields::new("executed", *executed_at, Some(tx_hash.clone()), None),
        DisbursementStatus::Finalized {
            tx_hash,
            finalized_at,
            ..
        } => StatusInsertFields::new("finalized", *finalized_at, Some(tx_hash.clone()), None),
        DisbursementStatus::RolledBack {
            reason,
            rolled_back_at,
            prior_tx,
        } => StatusInsertFields::new(
            "rolled_back",
            *rolled_back_at,
            prior_tx.clone(),
            Some(reason.clone()),
        ),
    }
}

fn legacy_status_from_label(
    label: &str,
    status_ts: u64,
    tx_hash: &Option<String>,
    cancel_reason: &Option<String>,
) -> DisbursementStatus {
    match label {
        "draft" => DisbursementStatus::Draft {
            created_at: status_ts,
        },
        "voting" => DisbursementStatus::Voting {
            vote_deadline_epoch: status_ts,
        },
        "queued" => DisbursementStatus::Queued {
            queued_at: status_ts,
            activation_epoch: status_ts,
        },
        "timelocked" => DisbursementStatus::Timelocked {
            ready_epoch: status_ts,
        },
        "executed" => DisbursementStatus::Executed {
            tx_hash: tx_hash.clone().unwrap_or_default(),
            executed_at: status_ts,
        },
        "finalized" => DisbursementStatus::Finalized {
            tx_hash: tx_hash.clone().unwrap_or_default(),
            executed_at: status_ts,
            finalized_at: status_ts,
        },
        "rolled_back" | "cancelled" => DisbursementStatus::RolledBack {
            reason: cancel_reason.clone().unwrap_or_default(),
            rolled_back_at: status_ts,
            prior_tx: tx_hash.clone(),
        },
        "scheduled" => DisbursementStatus::Draft {
            created_at: status_ts,
        },
        _ => DisbursementStatus::Draft {
            created_at: status_ts,
        },
    }
}

impl TreasuryDisbursementPage {
    fn to_json_value(&self) -> json::Value {
        let mut map = json::Map::new();
        map.insert(
            "total".into(),
            json::Value::Number(json::Number::from(self.total as u64)),
        );
        map.insert(
            "page".into(),
            json::Value::Number(json::Number::from(self.page as u64)),
        );
        map.insert(
            "page_size".into(),
            json::Value::Number(json::Number::from(self.page_size as u64)),
        );
        let disbursements = self
            .disbursements
            .iter()
            .map(TreasuryDisbursementRow::to_json_value)
            .collect();
        map.insert("disbursements".into(), json::Value::Array(disbursements));
        json::Value::Object(map)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreasuryDisbursementStatusFilter {
    Draft,
    Voting,
    Queued,
    Timelocked,
    Executed,
    Finalized,
    RolledBack,
    Scheduled,
    Cancelled,
}

#[derive(Debug, Clone, Default)]
pub struct TreasuryDisbursementFilter {
    pub status: Option<TreasuryDisbursementStatusFilter>,
    pub destination: Option<String>,
    pub min_epoch: Option<u64>,
    pub max_epoch: Option<u64>,
    pub min_amount: Option<u64>,
    pub max_amount: Option<u64>,
    pub min_created_at: Option<u64>,
    pub max_created_at: Option<u64>,
    pub min_status_ts: Option<u64>,
    pub max_status_ts: Option<u64>,
}

impl TreasuryDisbursementFilter {
    fn matches(&self, row: &TreasuryDisbursementRow) -> bool {
        if let Some(status) = self.status {
            let matches_status = match status {
                TreasuryDisbursementStatusFilter::Draft => {
                    matches!(row.status, DisbursementStatus::Draft { .. })
                }
                TreasuryDisbursementStatusFilter::Voting => {
                    matches!(row.status, DisbursementStatus::Voting { .. })
                }
                TreasuryDisbursementStatusFilter::Queued => {
                    matches!(row.status, DisbursementStatus::Queued { .. })
                }
                TreasuryDisbursementStatusFilter::Timelocked => {
                    matches!(row.status, DisbursementStatus::Timelocked { .. })
                }
                TreasuryDisbursementStatusFilter::Executed => {
                    matches!(row.status, DisbursementStatus::Executed { .. })
                }
                TreasuryDisbursementStatusFilter::Finalized => {
                    matches!(row.status, DisbursementStatus::Finalized { .. })
                }
                TreasuryDisbursementStatusFilter::RolledBack => {
                    matches!(row.status, DisbursementStatus::RolledBack { .. })
                }
                TreasuryDisbursementStatusFilter::Scheduled => matches!(
                    row.status,
                    DisbursementStatus::Draft { .. }
                        | DisbursementStatus::Voting { .. }
                        | DisbursementStatus::Queued { .. }
                        | DisbursementStatus::Timelocked { .. }
                ),
                TreasuryDisbursementStatusFilter::Cancelled => {
                    matches!(row.status, DisbursementStatus::RolledBack { .. })
                }
            };
            if !matches_status {
                return false;
            }
        }
        if let Some(dest) = &self.destination {
            if !row.destination.eq_ignore_ascii_case(dest) {
                return false;
            }
        }
        if let Some(min_epoch) = self.min_epoch {
            if row.scheduled_epoch < min_epoch {
                return false;
            }
        }
        if let Some(max_epoch) = self.max_epoch {
            if row.scheduled_epoch > max_epoch {
                return false;
            }
        }
        if let Some(min_amount) = self.min_amount {
            if row.amount < min_amount {
                return false;
            }
        }
        if let Some(max_amount) = self.max_amount {
            if row.amount > max_amount {
                return false;
            }
        }
        if let Some(min_created_at) = self.min_created_at {
            if row.created_at < min_created_at {
                return false;
            }
        }
        if let Some(max_created_at) = self.max_created_at {
            if row.created_at > max_created_at {
                return false;
            }
        }
        if let Some(min_status_ts) = self.min_status_ts {
            if row.status_timestamp < min_status_ts {
                return false;
            }
        }
        if let Some(max_status_ts) = self.max_status_ts {
            if row.status_timestamp > max_status_ts {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DidDocumentView {
    pub address: String,
    pub document: String,
    pub hash: String,
    pub nonce: u64,
    pub updated_at: u64,
    pub public_key: String,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_signer: Option<String>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_signature: Option<String>,
}

impl From<DidRecord> for DidDocumentView {
    fn from(record: DidRecord) -> Self {
        Self {
            address: record.address,
            document: record.document,
            hash: hex_encode(record.hash),
            nonce: record.nonce,
            updated_at: record.updated_at,
            public_key: hex_encode(record.public_key),
            remote_signer: record
                .remote_attestation
                .as_ref()
                .map(|att| att.signer.clone()),
            remote_signature: record
                .remote_attestation
                .as_ref()
                .map(|att| att.signature.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DidRecordRow {
    pub address: String,
    pub hash: String,
    pub anchored_at: i64,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub wallet_url: Option<String>,
}

pub struct Explorer {
    path: PathBuf,
    data_dir: PathBuf,
    gov_db_path: PathBuf,
    did: Mutex<DidRegistry>,
    did_cache: Mutex<LruCache<String, DidDocumentView>>,
}

impl Explorer {
    fn decode_block(bytes: &[u8]) -> AnyhowResult<Block> {
        if let Ok(block) = decode_json(bytes) {
            return Ok(block);
        }
        match binary::decode(bytes) {
            Ok(block) => Ok(block),
            Err(primary_err) => match the_block::block_binary::decode_block(bytes) {
                Ok(block) => Ok(block),
                Err(fallback_err) => Err(anyhow::anyhow!(
                    "decode block: {primary_err}; block_binary fallback: {fallback_err}"
                )),
            },
        }
    }

    fn decode_tx(bytes: &[u8]) -> AnyhowResult<SignedTransaction> {
        if let Ok(tx) = decode_json(bytes) {
            return Ok(tx);
        }
        binary::decode(bytes).map_err(|e| anyhow::anyhow!("decode tx: {e}"))
    }

    pub fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let p = path.as_ref().to_path_buf();
        let data_dir = env::var("TB_NODE_DATA_DIR").unwrap_or_else(|_| "node-data".into());
        let gov_db_path = env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into());
        let mut conn = Connection::open(&p)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS receipts (key TEXT PRIMARY KEY, epoch INTEGER, provider TEXT, buyer TEXT, amount INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (hash TEXT PRIMARY KEY, height INTEGER, data BLOB)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS txs (hash TEXT PRIMARY KEY, block_hash TEXT, memo TEXT, contract TEXT, data BLOB)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS gov (id INTEGER PRIMARY KEY, data BLOB)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peer_reputation (peer_id TEXT PRIMARY KEY, score INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peer_handshakes (peer_id TEXT PRIMARY KEY, success INTEGER, failure INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dex_orders (id INTEGER PRIMARY KEY AUTOINCREMENT, side TEXT, price INTEGER, amount INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS compute_jobs (job_id TEXT PRIMARY KEY, buyer TEXT, provider TEXT, status TEXT)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS compute_settlement (provider TEXT PRIMARY KEY, ct INTEGER, industrial INTEGER, updated_at INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS snark_proofs (job_id TEXT PRIMARY KEY, verified INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS compute_sla_history (job_id TEXT PRIMARY KEY, provider TEXT NOT NULL, buyer TEXT NOT NULL, outcome TEXT NOT NULL, outcome_reason TEXT, burned INTEGER NOT NULL, refunded INTEGER NOT NULL, deadline INTEGER NOT NULL, resolved_at INTEGER NOT NULL)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS compute_sla_proofs (job_id TEXT PRIMARY KEY, bundles BLOB NOT NULL)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS trust_lines (from_id TEXT, to_id TEXT, \"limit\" INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subsidy_history (epoch INTEGER PRIMARY KEY, beta INTEGER, gamma INTEGER, kappa INTEGER, lambda INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics_archive (name TEXT, ts INTEGER, value REAL)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS light_proofs (block_hash TEXT PRIMARY KEY, proof BLOB)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS token_supply (symbol TEXT, height INTEGER, supply INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bridge_volume (symbol TEXT, amount INTEGER, ts INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bridge_challenges (commitment TEXT PRIMARY KEY, user TEXT, amount INTEGER, challenged INTEGER, initiated_at INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS storage_contracts (object_id TEXT PRIMARY KEY, provider_id TEXT, price_per_block INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_stats (provider_id TEXT PRIMARY KEY, capacity_bytes INTEGER, reputation INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS release_history (hash TEXT PRIMARY KEY, proposer TEXT, activation_epoch INTEGER, install_count INTEGER)",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS treasury_disbursements (id INTEGER PRIMARY KEY, destination TEXT NOT NULL, amount INTEGER NOT NULL, memo TEXT NOT NULL, scheduled_epoch INTEGER NOT NULL, created_at INTEGER NOT NULL, status TEXT NOT NULL, status_ts INTEGER NOT NULL, tx_hash TEXT, cancel_reason TEXT, status_payload TEXT)",
            params![],
        )?;
        if let Err(err) = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_treasury_disbursements_status ON treasury_disbursements(status)",
            params![],
        ) {
            if !matches!(err, SqlError::Parse(_)) {
                return Err(err);
            }
        }
        if let Err(err) = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_treasury_disbursements_schedule ON treasury_disbursements(scheduled_epoch DESC, id DESC)",
            params![],
        ) {
            if !matches!(err, SqlError::Parse(_)) {
                return Err(err);
            }
        }
        conn.execute(
            "CREATE TABLE IF NOT EXISTS did_records (address TEXT NOT NULL, hash TEXT NOT NULL, anchored_at INTEGER NOT NULL, PRIMARY KEY(address, anchored_at))",
            params![],
        )?;
        if let Err(err) = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_did_records_address ON did_records(address)",
            params![],
        ) {
            if !matches!(err, SqlError::Parse(_)) {
                return Err(err);
            }
        }
        if let Err(err) = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_did_records_time ON did_records(anchored_at DESC)",
            params![],
        ) {
            if !matches!(err, SqlError::Parse(_)) {
                return Err(err);
            }
        }
        let did_registry = DidRegistry::open(DidRegistry::default_path());
        let mut cache = LruCache::new(NonZeroUsize::new(256).unwrap());
        {
            let seed_tx = conn.transaction()?;
            for view in did_registry
                .records()
                .into_iter()
                .map(DidDocumentView::from)
            {
                seed_tx.execute(
                    "INSERT OR REPLACE INTO did_records (address, hash, anchored_at) VALUES (?1, ?2, ?3)",
                    params![&view.address, &view.hash, view.updated_at as i64],
                )?;
                cache.put(view.address.clone(), view);
            }
            seed_tx.commit()?;
        }
        Ok(Self {
            path: p,
            data_dir: PathBuf::from(data_dir),
            gov_db_path: PathBuf::from(gov_db_path),
            did: Mutex::new(did_registry),
            did_cache: Mutex::new(cache),
        })
    }

    pub fn record_bridge_challenge(&self, rec: &BridgeChallengeRecord) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO bridge_challenges (commitment, user, amount, challenged, initiated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &rec.commitment,
                &rec.user,
                rec.amount as i64,
                if rec.challenged { 1 } else { 0 },
                rec.initiated_at,
            ],
        )?;
        Ok(())
    }

    pub fn active_bridge_challenges(&self) -> DbResult<Vec<BridgeChallengeRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT commitment, user, amount, challenged, initiated_at FROM bridge_challenges WHERE challenged = 0",
        )?;
        let rows = stmt.query_map(params![], |row| {
            Ok(BridgeChallengeRecord {
                commitment: row.get(0)?,
                user: row.get(1)?,
                amount: row.get::<_, i64>(2)? as u64,
                challenged: row.get::<_, i64>(3)? != 0,
                initiated_at: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn conn(&self) -> DbResult<Connection> {
        Connection::open(&self.path)
    }

    fn data_root(&self, override_dir: Option<&str>) -> PathBuf {
        override_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.data_dir.clone())
    }

    fn policy_snapshot_history(
        &self,
        data_dir_override: Option<&str>,
        start_epoch: Option<u64>,
        end_epoch: Option<u64>,
        limit: usize,
    ) -> io::Result<Vec<AdPolicySnapshotSummary>> {
        let base = self.data_root(data_dir_override).join("ad_policy");
        list_policy_snapshots(&base, start_epoch, end_epoch, limit)
    }

    fn policy_snapshot(
        &self,
        data_dir_override: Option<&str>,
        epoch: u64,
    ) -> io::Result<Option<AdPolicySnapshotDetail>> {
        let base = self.data_root(data_dir_override).join("ad_policy");
        read_policy_snapshot(&base, epoch)
    }

    fn readiness_status(
        &self,
        data_dir_override: Option<&str>,
        gov_override: Option<&str>,
    ) -> Result<Option<AdReadinessStatusView>, HttpError> {
        let data_root = self.data_root(data_dir_override);
        let gov_path = gov_override
            .map(PathBuf::from)
            .unwrap_or_else(|| self.gov_db_path.clone());
        let params = load_governance_params(&gov_path)?;
        let status = load_readiness_status(
            &data_root,
            // When a governance override is provided, prefer enabling rehearsal to surface the flag.
            if gov_override.is_some() {
                true
            } else {
                params.ad_rehearsal_enabled > 0
            },
            params.ad_rehearsal_stability_windows.max(0) as u64,
        )
        .map_err(|err| HttpError::Handler(format!("ad readiness status: {err}")))?;
        Ok(status)
    }

    fn tx_hash(tx: &SignedTransaction) -> String {
        let mut hasher = Hasher::new();
        let bytes = binary::encode(tx).unwrap_or_default();
        hasher.update(&bytes);
        hasher.finalize().to_hex().to_string()
    }

    pub fn index_block(&self, block: &Block) -> DbResult<()> {
        let conn = self.conn()?;
        let data = encode_json(block).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
            params![&block.hash, block.index, data],
        )?;
        for tx in &block.transactions {
            let hash = Self::tx_hash(tx);
            let memo = String::from_utf8(tx.payload.memo.clone()).unwrap_or_default();
            let contract = tx.payload.to.clone();
            let data = encode_json(tx).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO txs (hash, block_hash, memo, contract, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![hash, &block.hash, memo, contract, data],
            )?;
        }
        Ok(())
    }

    pub fn ingest_block_dir(&self, dir: &Path) -> DbResult<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for ent in entries.flatten() {
                if let Ok(bytes) = std::fs::read(ent.path()) {
                    if let Ok(block) = Self::decode_block(&bytes) {
                        let _ = self.index_block(&block);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_block(&self, hash: &str) -> DbResult<Option<Block>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE hash=?1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes.map(|b| Self::decode_block(&b).expect("failed to decode block from explorer db")))
    }

    pub fn get_block_by_height(&self, height: u64) -> DbResult<Option<Block>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE height=?1",
                params![height],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes.map(|b| Self::decode_block(&b).expect("failed to decode block from explorer db")))
    }

    pub fn block_payouts(&self, hash: &str) -> DbResult<Option<BlockPayoutBreakdown>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE hash=?1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;

        let Some(bytes) = bytes else {
            return Ok(None);
        };

        match Self::decode_block(&bytes) {
            Ok(block) => Ok(Some(BlockPayoutBreakdown::from_block(&block))),
            Err(err) => {
                if let Ok(value) = decode_json::<json::Value>(&bytes) {
                    if let Some(breakdown) = BlockPayoutBreakdown::from_json_with_hint(hash, &value)
                    {
                        return Ok(Some(breakdown));
                    }
                }
                log_error("failed to decode block payouts", &err);
                Ok(None)
            }
        }
    }

    pub fn block_hash_by_height(&self, height: u64) -> DbResult<Option<String>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT hash FROM blocks WHERE height=?1",
            params![height],
            |row| row.get(0),
        )
        .optional()
    }

    /// Fetch the base fee at the specified block height if present.
    pub fn base_fee_by_height(&self, height: u64) -> DbResult<Option<u64>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE height=?1",
                params![height],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes
            .and_then(|b| Self::decode_block(&b).ok())
            .map(|b| b.base_fee))
    }

    pub fn get_tx(&self, hash: &str) -> DbResult<Option<SignedTransaction>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row("SELECT data FROM txs WHERE hash=?1", params![hash], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(bytes.map(|b| Self::decode_tx(&b).unwrap()))
    }

    pub fn search_memo(&self, memo: &str) -> DbResult<Vec<SignedTransaction>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT data FROM txs WHERE memo LIKE ?1")?;
        let rows = stmt.query_map(params![memo], |row| row.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(bytes) = r {
                if let Ok(tx) = Self::decode_tx(&bytes) {
                    out.push(tx);
                }
            }
        }
        Ok(out)
    }

    pub fn search_contract(&self, contract: &str) -> DbResult<Vec<SignedTransaction>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT data FROM txs WHERE contract=?1")?;
        let rows = stmt.query_map(params![contract], |row| row.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(bytes) = r {
                if let Ok(tx) = Self::decode_tx(&bytes) {
                    out.push(tx);
                }
            }
        }
        Ok(out)
    }

    fn cache_hit(&self, address: &str) -> Option<DidDocumentView> {
        self.did_cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(address).cloned())
    }

    fn wallet_link(address: &str) -> String {
        format!("/wallets/{address}")
    }

    fn upsert_did_record(&self, view: &DidDocumentView) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO did_records (address, hash, anchored_at) VALUES (?1, ?2, ?3)",
            params![&view.address, &view.hash, view.updated_at as i64],
        )?;
        Ok(())
    }

    pub fn did_document(&self, address: &str) -> Option<DidDocumentView> {
        if let Some(hit) = self.cache_hit(address) {
            return Some(hit);
        }
        let record = {
            let guard = self.did.lock().ok()?;
            guard.resolve(address)
        }?;
        let view = DidDocumentView::from(record);
        if let Err(err) = self.upsert_did_record(&view) {
            eprintln!("persist DID record failed: {err}");
        }
        if let Ok(mut cache) = self.did_cache.lock() {
            cache.put(address.to_string(), view.clone());
        }
        Some(view)
    }

    pub fn record_did_anchor(&self, view: &DidDocumentView) -> DbResult<()> {
        self.upsert_did_record(view)?;
        if let Ok(mut cache) = self.did_cache.lock() {
            cache.put(view.address.clone(), view.clone());
        }
        Ok(())
    }

    pub fn recent_did_records(&self, limit: usize) -> DbResult<Vec<DidRecordRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT address, hash, anchored_at FROM did_records ORDER BY anchored_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(DidRecordRow {
                address: row.get(0)?,
                hash: row.get(1)?,
                anchored_at: row.get::<_, i64>(2)?,
                wallet_url: None,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            let mut rec = r?;
            rec.wallet_url = Some(Self::wallet_link(&rec.address));
            out.push(rec);
        }
        Ok(out)
    }

    pub fn did_records_for_address(&self, address: &str) -> DbResult<Vec<DidRecordRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT address, hash, anchored_at FROM did_records WHERE address=?1 ORDER BY anchored_at DESC",
        )?;
        let rows = stmt.query_map(params![address], |row| {
            Ok(DidRecordRow {
                address: row.get(0)?,
                hash: row.get(1)?,
                anchored_at: row.get::<_, i64>(2)?,
                wallet_url: None,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            let mut rec = r?;
            rec.wallet_url = Some(Self::wallet_link(&rec.address));
            out.push(rec);
        }
        Ok(out)
    }

    pub fn did_anchor_rate(&self) -> DbResult<Vec<MetricPoint>> {
        let mut points = self.metric_points("did_anchor_total")?;
        if points.len() < 2 {
            return Ok(Vec::new());
        }
        points.sort_by_key(|p| p.ts);
        let mut rates = Vec::new();
        for window in points.windows(2) {
            if let [prev, next] = window {
                let dt = (next.ts - prev.ts) as f64;
                if dt <= 0.0 {
                    continue;
                }
                let delta = next.value - prev.value;
                let rate = if delta <= 0.0 { 0.0 } else { delta / dt };
                rates.push(MetricPoint {
                    name: "did_anchor_rate".to_string(),
                    ts: next.ts,
                    value: rate,
                });
            }
        }
        Ok(rates)
    }

    pub fn index_gov_proposal(&self, prop: &GovProposal) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO gov (id, data) VALUES (?1, ?2)",
            params![prop.id, &prop.data],
        )?;
        Ok(())
    }

    pub fn get_gov_proposal(&self, id: u64) -> DbResult<Option<GovProposal>> {
        let conn = self.conn()?;
        let row: Option<(u64, Vec<u8>)> = conn
            .query_row("SELECT id, data FROM gov WHERE id=?1", params![id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        Ok(row.map(|(id, data)| GovProposal { id, data }))
    }

    pub fn set_peer_reputation(&self, peer_id: &str, score: i64) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO peer_reputation (peer_id, score) VALUES (?1, ?2)",
            params![peer_id, score],
        )?;
        Ok(())
    }

    pub fn peer_reputations(&self) -> DbResult<Vec<PeerReputation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT peer_id, score FROM peer_reputation")?;
        let rows = stmt.query_map(params![], |row| {
            Ok(PeerReputation {
                peer_id: row.get(0)?,
                score: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_order_book(&self, book: &OrderBook) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM dex_orders", params![])?;
        for (price, orders) in &book.bids {
            for ord in orders {
                conn.execute(
                    "INSERT INTO dex_orders (side, price, amount) VALUES ('buy', ?1, ?2)",
                    params![*price, ord.amount],
                )?;
            }
        }
        for (price, orders) in &book.asks {
            for ord in orders {
                conn.execute(
                    "INSERT INTO dex_orders (side, price, amount) VALUES ('sell', ?1, ?2)",
                    params![*price, ord.amount],
                )?;
            }
        }
        Ok(())
    }

    pub fn order_book(&self) -> DbResult<Vec<OrderRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT side, price, amount FROM dex_orders ORDER BY price")?;
        let rows = stmt.query_map(params![], |row| {
            Ok(OrderRecord {
                side: row.get(0)?,
                price: row.get(1)?,
                amount: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_job(&self, job: &Job, provider: &str, status: &str) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO compute_jobs (job_id, buyer, provider, status) VALUES (?1, ?2, ?3, ?4)",
            params![&job.job_id, &job.buyer, provider, status],
        )?;
        Ok(())
    }

    pub fn compute_jobs(&self) -> DbResult<Vec<ComputeJobRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT job_id, buyer, provider, status FROM compute_jobs")?;
        let rows = stmt.query_map(params![], |row| {
            Ok(ComputeJobRecord {
                job_id: row.get(0)?,
                buyer: row.get(1)?,
                provider: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_settlement_balances(&self, balances: &[ProviderSettlementRecord]) -> DbResult<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for bal in balances {
            let ct = i64::try_from(bal.ct).unwrap_or(i64::MAX);
            let industrial = i64::try_from(bal.industrial).unwrap_or(i64::MAX);
            tx.execute(
                "INSERT OR REPLACE INTO compute_settlement (provider, ct, industrial, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![&bal.provider, ct, industrial, bal.updated_at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn settlement_balances(&self) -> DbResult<Vec<ProviderSettlementRecord>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT provider, ct, industrial, updated_at FROM compute_settlement")?;
        let rows = stmt.query_map(params![], |row| {
            let ct: i64 = row.get(1)?;
            let industrial: i64 = row.get(2)?;
            Ok(ProviderSettlementRecord {
                provider: row.get(0)?,
                ct: ct.max(0) as u64,
                industrial: industrial.max(0) as u64,
                updated_at: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn record_sla_history(&self, entries: &[SlaResolution]) -> DbResult<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for entry in entries {
            let (outcome, reason) = sla_outcome_fields(&entry.outcome);
            tx.execute(
                "INSERT OR REPLACE INTO compute_sla_history (job_id, provider, buyer, outcome, outcome_reason, burned, refunded, deadline, resolved_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    &entry.job_id,
                    &entry.provider,
                    &entry.buyer,
                    outcome,
                    reason.unwrap_or(""),
                    clamp_i64(entry.burned),
                    clamp_i64(entry.refunded),
                    clamp_i64(entry.deadline),
                    clamp_i64(entry.resolved_at),
                ],
            )?;
            let bundles = binary::encode(&entry.proofs)?;
            tx.execute(
                "INSERT OR REPLACE INTO compute_sla_proofs (job_id, bundles) VALUES (?1, ?2)",
                params![&entry.job_id, bundles],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn compute_sla_history(&self, limit: usize) -> DbResult<Vec<ComputeSlaHistoryRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT job_id, provider, buyer, outcome, outcome_reason, burned, refunded, deadline, resolved_at FROM compute_sla_history ORDER BY resolved_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![clamp_i64(limit as u64)], |row| {
            Ok(ComputeSlaHistoryRecord {
                job_id: row.get(0)?,
                provider: row.get(1)?,
                buyer: row.get(2)?,
                outcome: row.get(3)?,
                outcome_reason: {
                    let reason: String = row.get(4)?;
                    if reason.is_empty() {
                        None
                    } else {
                        Some(reason)
                    }
                },
                burned: row.get::<_, i64>(5)? as u64,
                refunded: row.get::<_, i64>(6)? as u64,
                deadline: row.get::<_, i64>(7)? as u64,
                resolved_at: row.get::<_, i64>(8)? as u64,
                proofs: Vec::new(),
            })
        })?;
        let mut out = Vec::new();
        for rec in rows {
            let mut record = rec?;
            record.proofs = self.load_sla_proofs(&conn, &record.job_id)?;
            out.push(record);
        }
        Ok(out)
    }

    fn load_sla_proofs(
        &self,
        conn: &Connection,
        job_id: &str,
    ) -> DbResult<Vec<ComputeSlaProofRecord>> {
        let mut stmt = conn.prepare("SELECT bundles FROM compute_sla_proofs WHERE job_id=?1")?;
        let rows = stmt.query_map(params![job_id], |row| row.get::<_, Vec<u8>>(0))?;
        if let Some(row) = rows.into_iter().next() {
            let blob = row?;
            let bundles: Vec<ProofBundle> = binary::decode(&blob)?;
            let proofs: Vec<ComputeSlaProofRecord> =
                bundles.iter().map(proof_bundle_to_record).collect();
            Ok(proofs)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn index_trust_line(&self, from: &str, to: &str, limit: u64) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO trust_lines (from_id, to_id, \"limit\") VALUES (?1, ?2, ?3)",
            params![from, to, limit],
        )?;
        Ok(())
    }

    pub fn trust_lines(&self) -> DbResult<Vec<TrustLineRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT from_id, to_id, \"limit\" FROM trust_lines")?;
        let rows = stmt.query_map(params![], |row| {
            Ok(TrustLineRecord {
                from: row.get(0)?,
                to: row.get(1)?,
                limit: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_subsidy(&self, rec: &SubsidyRecord) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO subsidy_history (epoch, beta, gamma, kappa, lambda) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![rec.epoch, rec.beta, rec.gamma, rec.kappa, rec.lambda],
        )?;
        Ok(())
    }

    pub fn subsidy_history(&self) -> DbResult<Vec<SubsidyRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT epoch, beta, gamma, kappa, lambda FROM subsidy_history ORDER BY epoch",
        )?;
        let rows = stmt.query_map(params![], |row| {
            Ok(SubsidyRecord {
                epoch: row.get(0)?,
                beta: row.get(1)?,
                gamma: row.get(2)?,
                kappa: row.get(3)?,
                lambda: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn archive_metric(&self, point: &MetricPoint) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO metrics_archive (name, ts, value) VALUES (?1, ?2, ?3)",
            params![&point.name, point.ts, point.value],
        )?;
        Ok(())
    }

    pub fn metric_points(&self, name: &str) -> DbResult<Vec<MetricPoint>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT name, ts, value FROM metrics_archive WHERE name=?1 ORDER BY ts")?;
        let rows = stmt.query_map(params![name], |row| {
            Ok(MetricPoint {
                name: row.get(0)?,
                ts: row.get(1)?,
                value: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn fee_floor_history(&self) -> DbResult<Vec<FeeFloorPoint>> {
        let points = self.metric_points("fee_floor_current")?;
        Ok(points
            .into_iter()
            .map(|p| FeeFloorPoint {
                ts: p.ts,
                floor: p.value,
            })
            .collect())
    }

    pub fn index_storage_contract(&self, contract: &storage::StorageContract) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO storage_contracts (object_id, provider_id, price_per_block) VALUES (?1, ?2, ?3)",
            params![&contract.object_id, &contract.provider_id, contract.price_per_block],
        )?;
        Ok(())
    }

    pub fn set_provider_stats(
        &self,
        provider_id: &str,
        capacity_bytes: u64,
        reputation: i64,
    ) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO provider_stats (provider_id, capacity_bytes, reputation) VALUES (?1, ?2, ?3)",
            params![provider_id, capacity_bytes, reputation],
        )?;
        Ok(())
    }

    pub fn provider_storage_stats(&self) -> DbResult<Vec<ProviderStorageStat>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT ps.provider_id, ps.capacity_bytes, ps.reputation, COUNT(sc.object_id) as contracts \
             FROM provider_stats ps LEFT JOIN storage_contracts sc ON sc.provider_id = ps.provider_id \
             GROUP BY ps.provider_id, ps.capacity_bytes, ps.reputation",
        )?;
        let rows = stmt.query_map(params![], |row| {
            Ok(ProviderStorageStat {
                provider_id: row.get(0)?,
                capacity_bytes: row.get(1)?,
                reputation: row.get(2)?,
                contracts: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn manifest_listing(&self, limit: Option<usize>) -> storage_view::ManifestListingView {
        let path = env::var("TB_STORAGE_PIPELINE_DIR").unwrap_or_else(|_| "blobstore".to_string());
        let pipeline = the_block::storage::pipeline::StoragePipeline::open(&path);
        let max_entries = limit.unwrap_or(100).min(1_000);
        let manifests = pipeline.manifest_summaries(max_entries);
        let algorithms = the_block::storage::settings::algorithms();
        let policy = storage_view::ManifestPolicyView {
            erasure: storage_view::AlgorithmPolicyView {
                algorithm: algorithms.erasure().to_string(),
                fallback: algorithms.erasure_fallback(),
                emergency: algorithms.erasure_emergency(),
            },
            compression: storage_view::AlgorithmPolicyView {
                algorithm: algorithms.compression().to_string(),
                fallback: algorithms.compression_fallback(),
                emergency: algorithms.compression_emergency(),
            },
        };
        storage_view::render_manifest_listing(&manifests, policy)
    }

    pub fn index_light_proof(&self, proof: &LightProof) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO light_proofs (block_hash, proof) VALUES (?1, ?2)",
            params![&proof.block_hash, &proof.proof],
        )?;
        Ok(())
    }

    pub fn light_proof(&self, hash: &str) -> DbResult<Option<LightProof>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT block_hash, proof FROM light_proofs WHERE block_hash=?1",
            params![hash],
            |row| {
                Ok(LightProof {
                    block_hash: row.get(0)?,
                    proof: row.get(1)?,
                })
            },
        )
        .optional()
    }

    pub fn record_handshake(&self, peer_id: &str, success: bool) -> DbResult<()> {
        let conn = self.conn()?;
        let (succ, fail) = if success { (1, 0) } else { (0, 1) };
        conn.execute(
            "INSERT INTO peer_handshakes (peer_id, success, failure) VALUES (?1, ?2, ?3) \
             ON CONFLICT(peer_id) DO UPDATE SET success = success + excluded.success, failure = failure + excluded.failure",
            params![peer_id, succ, fail],
        )?;
        Ok(())
    }

    pub fn peer_handshakes(&self) -> DbResult<Vec<PeerHandshake>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT peer_id, success, failure FROM peer_handshakes")?;
        let rows = stmt.query_map(params![], |row| {
            Ok(PeerHandshake {
                peer_id: row.get(0)?,
                success: row.get(1)?,
                failure: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn record_token_supply(&self, symbol: &str, height: u64, supply: u64) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO token_supply (symbol, height, supply) VALUES (?1, ?2, ?3)",
            params![symbol, height, supply],
        )?;
        Ok(())
    }

    pub fn record_bridge_volume(&self, symbol: &str, amount: u64, ts: i64) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO bridge_volume (symbol, amount, ts) VALUES (?1, ?2, ?3)",
            params![symbol, amount, ts],
        )?;
        Ok(())
    }

    pub fn token_supply(&self, symbol: &str) -> DbResult<Vec<TokenSupplyRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT symbol, height, supply FROM token_supply WHERE symbol = ?1 ORDER BY height",
        )?;
        let rows = stmt.query_map(params![symbol], |row| {
            Ok(TokenSupplyRecord {
                symbol: row.get(0)?,
                height: row.get(1)?,
                supply: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn bridge_volume(&self, symbol: &str) -> DbResult<Vec<BridgeVolumeRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT symbol, amount, ts FROM bridge_volume WHERE symbol = ?1 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![symbol], |row| {
            Ok(BridgeVolumeRecord {
                symbol: row.get(0)?,
                amount: row.get(1)?,
                ts: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn record_release_entries(
        &self,
        entries: &[release_view::ReleaseHistoryEntry],
    ) -> DbResult<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for entry in entries {
            tx.execute(
                "INSERT OR REPLACE INTO release_history (hash, proposer, activation_epoch, install_count) VALUES (?1, ?2, ?3, ?4)",
                params![
                    &entry.build_hash,
                    &entry.proposer,
                    entry.activated_epoch as i64,
                    entry.install_count as i64
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn release_timeline(&self, gov_path: impl AsRef<Path>) -> AnyhowResult<Vec<(u64, usize)>> {
        let entries = release_view::release_history(gov_path)?;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.activated_epoch, entry.install_count))
            .collect())
    }

    pub fn index_treasury_disbursements(&self, records: &[TreasuryDisbursement]) -> DbResult<()> {
        let mut conn = self.conn()?;
        let _ = conn.execute(
            "ALTER TABLE treasury_disbursements ADD COLUMN status_payload TEXT",
            params![],
        );
        let _ = conn.execute(
            "ALTER TABLE treasury_disbursements RENAME COLUMN amount_ct TO amount",
            params![],
        );
        let _ = conn.execute(
            "ALTER TABLE treasury_disbursements DROP COLUMN amount_it",
            params![],
        );
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM treasury_disbursements", params![])?;
        for record in records {
            let fields = derive_status_fields(&record.status);
            let status_payload = json::to_string(&record.status).unwrap_or_else(|_| "{}".into());
            tx.execute(
                "INSERT OR REPLACE INTO treasury_disbursements (id, destination, amount, memo, scheduled_epoch, created_at, status, status_ts, tx_hash, cancel_reason, status_payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    record.id as i64,
                    &record.destination,
                    record.amount as i64,
                    &record.memo,
                    record.scheduled_epoch as i64,
                    record.created_at as i64,
                    fields.label,
                    fields.timestamp as i64,
                    optional_text_value(fields.tx_hash.as_deref()),
                    optional_text_value(fields.reason.as_deref()),
                    status_payload,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn treasury_disbursements(
        &self,
        page: usize,
        page_size: usize,
        filter: TreasuryDisbursementFilter,
    ) -> DbResult<TreasuryDisbursementPage> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, destination, amount, memo, scheduled_epoch, created_at, status, status_ts, tx_hash, cancel_reason, status_payload FROM treasury_disbursements ORDER BY scheduled_epoch DESC, id DESC")?;
        let rows = stmt.query_map(params![], |row| {
            let status_text: String = row.get(6)?;
            let status_ts: i64 = row.get(7)?;
            let tx_hash: Option<String> = row.get(8)?;
            let cancel_reason: Option<String> = row.get(9)?;
            let status_payload: Option<String> = row.get(10)?;
            let status = if let Some(payload) = status_payload {
                json::from_str(&payload).unwrap_or_else(|_| {
                    legacy_status_from_label(
                        &status_text,
                        status_ts.max(0) as u64,
                        &tx_hash,
                        &cancel_reason,
                    )
                })
            } else {
                legacy_status_from_label(
                    &status_text,
                    status_ts.max(0) as u64,
                    &tx_hash,
                    &cancel_reason,
                )
            };
            let executed_tx_hash = if matches!(status, DisbursementStatus::Executed { .. }) {
                tx_hash.clone()
            } else {
                None
            };
            let cancel_text = if matches!(status, DisbursementStatus::RolledBack { .. }) {
                cancel_reason.clone()
            } else {
                None
            };
            Ok(TreasuryDisbursementRow {
                id: row.get::<_, i64>(0)? as u64,
                destination: row.get(1)?,
                amount: row.get::<_, i64>(2)? as u64,
                memo: row.get(3)?,
                scheduled_epoch: row.get::<_, i64>(4)? as u64,
                created_at: row.get::<_, i64>(5)? as u64,
                status_label: status_text,
                status_timestamp: status_ts.max(0) as u64,
                status,
                executed_tx_hash,
                cancel_reason: cancel_text,
            })
        })?;
        let mut records = Vec::new();
        for entry in rows {
            records.push(entry?);
        }
        records.sort_by(|a, b| {
            b.scheduled_epoch
                .cmp(&a.scheduled_epoch)
                .then_with(|| b.id.cmp(&a.id))
        });
        let filtered: Vec<TreasuryDisbursementRow> = records
            .into_iter()
            .filter(|row| filter.matches(row))
            .collect();
        let total = filtered.len();
        let size = page_size.max(1);
        let start = page.saturating_mul(size);
        let end = (start + size).min(total);
        let disbursements = if start >= total {
            Vec::new()
        } else {
            filtered[start..end].to_vec()
        };
        Ok(TreasuryDisbursementPage {
            total,
            page,
            page_size: size,
            disbursements,
        })
    }

    pub fn ingest_dir(&self, dir: &Path) -> DbResult<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for ent in entries.flatten() {
                if let Ok(epoch) = ent.file_name().to_string_lossy().parse::<u64>() {
                    if let Ok(bytes) = std::fs::read(ent.path()) {
                        if let Ok(list) = binary::decode::<Vec<Receipt>>(&bytes) {
                            for r in list {
                                let rec = ReceiptRecord {
                                    key: hex_encode(r.idempotency_key),
                                    epoch,
                                    provider: r.provider.clone(),
                                    buyer: r.buyer.clone(),
                                    amount: r.quote_price,
                                };
                                let _ = self.index_receipt(&rec);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn index_receipt(&self, rec: &ReceiptRecord) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO receipts (key, epoch, provider, buyer, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&rec.key, rec.epoch, &rec.provider, &rec.buyer, rec.amount],
        )?;
        Ok(())
    }

    pub fn receipts_by_provider(&self, prov: &str) -> DbResult<Vec<ReceiptRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, epoch, provider, buyer, amount FROM receipts WHERE provider=?1 ORDER BY epoch",
        )?;
        let rows = stmt.query_map(params![prov], |row| {
            Ok(ReceiptRecord {
                key: row.get(0)?,
                epoch: row.get(1)?,
                provider: row.get(2)?,
                buyer: row.get(3)?,
                amount: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn receipts_by_domain(&self, dom: &str) -> DbResult<Vec<ReceiptRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, epoch, provider, buyer, amount FROM receipts WHERE buyer=?1 ORDER BY epoch",
        )?;
        let rows = stmt.query_map(params![dom], |row| {
            Ok(ReceiptRecord {
                key: row.get(0)?,
                epoch: row.get(1)?,
                provider: row.get(2)?,
                buyer: row.get(3)?,
                amount: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Load opcode trace for a transaction from `trace/<hash>.json`.
    pub fn opcode_trace(&self, tx_hash: &str) -> Option<Vec<String>> {
        let path = PathBuf::from("trace").join(format!("{tx_hash}.json"));
        std::fs::read(path).ok().and_then(|b| decode_json(&b).ok())
    }

    /// Convert WASM bytecode into a human-readable summary understood by the
    /// first-party interpreter.
    pub fn wasm_disasm(&self, bytes: &[u8]) -> Option<String> {
        let meta = the_block::vm::wasm::analyze(bytes).ok()?;
        let instructions = the_block::vm::wasm::disassemble(bytes).ok()?;
        let mut out = String::new();
        out.push_str(&format!("version: {}\n", meta.version));
        out.push_str(&format!("instructions: {}\n", meta.instruction_count));
        out.push_str(&format!("required_inputs: {}\n", meta.required_inputs));
        if let Some(ret) = meta.return_values {
            out.push_str(&format!("return_values: {}\n", ret));
        }
        out.push_str("code:\n");
        for (idx, instr) in instructions.iter().enumerate() {
            use the_block::vm::wasm::Instruction::*;
            let line = match instr {
                Nop => "nop".to_string(),
                PushConst(v) => format!("push_const {v}"),
                PushInput(i) => format!("push_input {i}"),
                Add => "add".to_string(),
                Sub => "sub".to_string(),
                Mul => "mul".to_string(),
                Div => "div".to_string(),
                Eq => "eq".to_string(),
                Return(count) => format!("return {count}"),
            };
            out.push_str(&format!("  {idx:04}: {line}\n"));
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile;

    #[test]
    fn index_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("explorer.db");
        let ex = Explorer::open(&db).unwrap();
        ex.index_receipt(&ReceiptRecord {
            key: "key-1".into(),
            epoch: 1,
            provider: "prov".into(),
            buyer: "buyer".into(),
            amount: 10,
        })
        .unwrap();
        assert_eq!(ex.receipts_by_provider("prov").unwrap().len(), 1);
        assert_eq!(ex.receipts_by_domain("buyer").unwrap().len(), 1);
    }
}
