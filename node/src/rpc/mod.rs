use crate::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    consensus::pow::{self, BlockHeader},
    gateway,
    governance::{GovStore, Params},
    identity::{handle_registry::HandleRegistry, DidRegistry},
    kyc,
    localnet::{validate_proximity, AssistReceipt},
    net, range_boost,
    simple_db::{names, SimpleDb},
    storage::fs::RentEscrow,
    transaction::FeeLane,
    Blockchain, SignedTransaction,
};
use ::storage::{contract::StorageContract, offer::StorageOffer};
use base64_fp::decode_standard;
use concurrency::Lazy;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{binary, Deserialize, Serialize};
use hex;
use httpd::{
    form_urlencoded, serve, HttpError, Method, Request, Response, Router, ServerConfig, StatusCode,
    WebSocketRequest, WebSocketResponse,
};
use runtime::net::TcpListener;
use runtime::sync::{
    oneshot,
    semaphore::{OwnedSemaphorePermit, Semaphore},
};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use subtle::ConstantTimeEq;

pub mod ledger;
pub mod limiter;
pub mod scheduler;
use limiter::{ClientState, RpcClientErrorCode};

#[cfg(feature = "telemetry")]
pub mod analytics;
pub mod bridge;
pub mod client;
pub mod compute_market;
pub mod consensus;
pub mod dex;
pub mod governance;
pub mod htlc;
pub mod identity;
pub mod inflation;
pub mod jurisdiction;
pub mod light;
pub mod logs;
pub mod peer;
pub mod pos;
pub mod state_stream;
pub mod storage;
pub mod vm;
pub mod vm_trace;

static GOV_STORE: Lazy<GovStore> = Lazy::new(|| GovStore::open("governance_db"));
static GOV_PARAMS: Lazy<Mutex<Params>> = Lazy::new(|| Mutex::new(Params::default()));
static LOCALNET_RECEIPTS: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_LOCALNET_DB_PATH").unwrap_or_else(|_| "localnet_db".into());
    Mutex::new(SimpleDb::open_named(names::LOCALNET_RECEIPTS, &path))
});

struct RpcState {
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
    clients: Arc<Mutex<HashMap<IpAddr, ClientState>>>,
    tokens_per_sec: f64,
    ban_secs: u64,
    client_timeout: u64,
    concurrent: Arc<Semaphore>,
}

impl RpcState {
    async fn acquire(&self) -> Result<OwnedSemaphorePermit, HttpError> {
        self.concurrent
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| HttpError::Handler("rpc semaphore closed".into()))
    }

    fn check_rate_limit(&self, remote: SocketAddr) -> Option<Response> {
        match limiter::check_client(
            &remote.ip(),
            &self.clients,
            self.tokens_per_sec,
            self.ban_secs,
            self.client_timeout,
        ) {
            Ok(_) => None,
            Err(code) => {
                telemetry_rpc_error(code);
                let err = RpcError {
                    code: code.rpc_code(),
                    message: code.message(),
                };
                let resp = RpcResponse::Error {
                    jsonrpc: "2.0",
                    error: err,
                    id: None,
                };
                let response = Response::new(StatusCode::TOO_MANY_REQUESTS)
                    .json(&resp)
                    .unwrap_or_else(|_| {
                        Response::new(StatusCode::TOO_MANY_REQUESTS).with_body(b"{}".to_vec())
                    })
                    .close();
                Some(response)
            }
        }
    }

    fn apply_cors(&self, mut response: Response, origin: Option<&str>) -> Response {
        if let Some(origin) = origin {
            if self
                .runtime_cfg
                .cors_allow_origins
                .iter()
                .any(|allowed| allowed == origin)
            {
                response = response.with_header("access-control-allow-origin", origin);
            }
        }
        response
    }

    fn is_host_allowed(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return false;
        };
        self.runtime_cfg
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host.trim()))
    }

    fn runtime(&self) -> Arc<RpcRuntimeConfig> {
        Arc::clone(&self.runtime_cfg)
    }
}

fn render_request_path(request: &Request<RpcState>) -> String {
    if request.query().is_empty() {
        request.path().to_string()
    } else {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        let mut pairs: Vec<_> = request.query().iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in pairs {
            serializer.append_pair(key, value);
        }
        format!("{}?{}", request.path(), serializer.finish())
    }
}

pub struct RpcRuntimeConfig {
    allowed_hosts: Vec<String>,
    cors_allow_origins: Vec<String>,
    max_body_bytes: usize,
    request_timeout: Duration,
    enable_debug: bool,
    admin_token: Option<String>,
    relay_only: bool,
}

const PUBLIC_METHODS: &[&str] = &[
    "balance",
    "register_handle",
    "resolve_handle",
    "whoami",
    "identity.anchor",
    "identity.resolve",
    "submit_tx",
    "tx_status",
    "price_board_get",
    "metrics",
    "settlement_status",
    "settlement.audit",
    "localnet.submit_receipt",
    "record_le_request",
    "warrant_canary",
    "le.list_requests",
    "le.record_action",
    "le.upload_evidence",
    "service_badge_issue",
    "service_badge_revoke",
    "dns.publish_record",
    "gateway.policy",
    "gateway.reads_since",
    "gov.release_signers",
    #[cfg(feature = "telemetry")]
    "analytics",
    "microshard.roots.last",
    "mempool.stats",
    "mempool.qos_event",
    "net.peer_stats",
    "net.peer_stats_all",
    "net.peer_stats_reset",
    "net.peer_stats_export",
    "net.peer_stats_export_all",
    "net.peer_stats_persist",
    "net.peer_throttle",
    "net.overlay_status",
    "net.backpressure_clear",
    "net.reputation_sync",
    "net.reputation_show",
    "net.gossip_status",
    "net.dns_verify",
    "net.key_rotate",
    "net.handshake_failures",
    "net.quic_stats",
    "net.quic_certs",
    "net.quic_certs_refresh",
    "kyc.verify",
    "pow.get_template",
    "dex_escrow_status",
    "dex_escrow_release",
    "dex_escrow_proof",
    "htlc_status",
    "htlc_refund",
    "storage_upload",
    "storage_challenge",
    "storage.repair_history",
    "storage.repair_run",
    "storage.repair_chunk",
    "storage.manifests",
    "pow.submit",
    "inflation.params",
    "compute_market.stats",
    "compute_market.provider_balances",
    "compute_market.audit",
    "compute_market.recent_roots",
    "compute_market.scheduler_metrics",
    "compute_market.scheduler_stats",
    "compute.reputation_get",
    "compute.job_requirements",
    "compute.job_cancel",
    "compute.provider_hardware",
    "stake.role",
    "consensus.difficulty",
    "consensus.pos.register",
    "config.reload",
    "consensus.pos.bond",
    "consensus.pos.unbond",
    "consensus.pos.slash",
    "rent.escrow.balance",
    "mesh.peers",
    "light.latest_header",
    "light.headers",
    "light_client.rebate_status",
    "light_client.rebate_history",
    "service_badge_verify",
    "jurisdiction.status",
    "jurisdiction.policy_diff",
];

const ADMIN_METHODS: &[&str] = &[
    "set_snapshot_interval",
    "compute_arm_real",
    "compute_cancel_arm",
    "compute_back_to_dry_run",
    "gateway.mobile_cache_status",
    "gateway.mobile_cache_flush",
    "telemetry.configure",
    "gov_propose",
    "gov_vote",
    "submit_proposal",
    "vote_proposal",
    "gov_list",
    "gov_params",
    "gov_rollback_last",
    "gov_rollback",
    "jurisdiction.set",
];

const BADGE_METHODS: &[&str] = &[
    "record_le_request",
    "warrant_canary",
    "le.list_requests",
    "le.record_action",
    "le.upload_evidence",
];

const DEBUG_METHODS: &[&str] = &["set_difficulty", "start_mining", "stop_mining"];

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    _jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: foundation_serialization::json::Value,
    #[serde(default)]
    id: Option<foundation_serialization::json::Value>,
    #[serde(default)]
    badge: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RpcResponse {
    Result {
        jsonrpc: &'static str,
        result: foundation_serialization::json::Value,
        id: Option<foundation_serialization::json::Value>,
    },
    Error {
        jsonrpc: &'static str,
        error: RpcError,
        id: Option<foundation_serialization::json::Value>,
    },
}

#[derive(Serialize, Debug)]
pub struct RpcError {
    code: i32,
    message: &'static str,
}

impl RpcError {
    pub fn code(&self) -> i32 {
        self.code
    }

    pub fn message(&self) -> &'static str {
        self.message
    }
}

fn parse_overlay_peer_param(id: &str) -> Result<[u8; 32], RpcError> {
    if id.is_empty() {
        return Err(RpcError {
            code: -32602,
            message: "invalid params",
        });
    }
    if let Ok(peer) = net::overlay_peer_from_base58(id) {
        let mut out = [0u8; 32];
        out.copy_from_slice(peer.as_bytes());
        return Ok(out);
    }
    if let Ok(bytes) = hex::decode(id) {
        if let Ok(peer) = net::overlay_peer_from_bytes(&bytes) {
            let mut out = [0u8; 32];
            out.copy_from_slice(peer.as_bytes());
            return Ok(out);
        }
    }
    Err(RpcError {
        code: -32602,
        message: "invalid params",
    })
}

fn io_err_msg(e: &std::io::Error) -> &'static str {
    if e.to_string().contains("peer list changed") {
        "peer list changed"
    } else if e.to_string().contains("quota exceeded") {
        "quota exceeded"
    } else if e.kind() == std::io::ErrorKind::InvalidInput {
        "invalid path"
    } else {
        "export failed"
    }
}

#[derive(Debug)]
enum SnapshotError {
    IntervalTooSmall,
}

impl SnapshotError {
    fn code(&self) -> i32 {
        -32050
    }
    fn message(&self) -> &'static str {
        "interval too small"
    }
}

impl From<SnapshotError> for RpcError {
    fn from(e: SnapshotError) -> Self {
        Self {
            code: e.code(),
            message: e.message(),
        }
    }
}

impl From<crate::compute_market::MarketError> for RpcError {
    fn from(e: crate::compute_market::MarketError) -> Self {
        Self {
            code: e.code(),
            message: e.message(),
        }
    }
}

#[doc(hidden)]

fn telemetry_rpc_error(code: RpcClientErrorCode) {
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::RPC_CLIENT_ERROR_TOTAL
            .with_label_values(&[code.as_str()])
            .inc();
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = code;
}

fn check_nonce(
    scope: impl Into<String>,
    params: &foundation_serialization::json::Value,
    nonces: &Arc<Mutex<HashSet<(String, u64)>>>,
) -> Result<(), RpcError> {
    let nonce = params
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or(RpcError {
            code: -32602,
            message: "missing nonce",
        })?;
    let key = scope.into();
    let mut guard = nonces.lock().map_err(|_| RpcError {
        code: -32603,
        message: "internal error",
    })?;
    if !guard.insert((key, nonce)) {
        return Err(RpcError {
            code: -32000,
            message: "replayed nonce",
        });
    }
    Ok(())
}

fn execute_rpc(
    state: &RpcState,
    request: RpcRequest,
    auth: Option<&str>,
    peer_ip: Option<IpAddr>,
) -> RpcResponse {
    let runtime_cfg = state.runtime();
    let id = request.id.clone();
    let method_str = request.method.as_str();
    let authorized = match runtime_cfg.admin_token.as_ref() {
        Some(token) => {
            let expected = format!("Bearer {token}");
            auth.map(|h| {
                let a = h.as_bytes();
                let b = expected.as_bytes();
                a.len() == b.len() && a.ct_eq(b).into()
            })
            .unwrap_or(false)
        }
        None => false,
    };

    let dispatch_result = || {
        dispatch(
            &request,
            Arc::clone(&state.bc),
            Arc::clone(&state.mining),
            Arc::clone(&state.nonces),
            Arc::clone(&state.handles),
            Arc::clone(&state.dids),
            Arc::clone(&runtime_cfg),
        )
    };

    let response = if DEBUG_METHODS.contains(&method_str) {
        if !runtime_cfg.enable_debug || !authorized {
            Err(RpcError {
                code: -32601,
                message: "method not found",
            })
        } else {
            dispatch_result()
        }
    } else if ADMIN_METHODS.contains(&method_str) {
        if !authorized {
            Err(RpcError {
                code: -32601,
                message: "method not found",
            })
        } else {
            dispatch_result()
        }
    } else if PUBLIC_METHODS.contains(&method_str) {
        if matches!(
            method_str,
            "net.peer_stats"
                | "net.peer_stats_all"
                | "net.peer_stats_reset"
                | "net.peer_stats_export"
                | "net.peer_stats_export_all"
                | "net.peer_throttle"
                | "net.backpressure_clear"
        ) {
            let local = peer_ip.map(|ip| ip.is_loopback()).unwrap_or(false);
            if !local {
                Err(RpcError {
                    code: -32601,
                    message: "method not found",
                })
            } else {
                if method_str == "net.peer_stats_export" {
                    #[cfg(feature = "telemetry")]
                    {
                        if let Some(path) = request.params.get("path").and_then(|v| v.as_str()) {
                            diagnostics::log::info!(
                                "peer_stats_export operator={peer_ip:?} path={path}"
                            );
                        } else {
                            diagnostics::log::info!("peer_stats_export operator={peer_ip:?}");
                        }
                    }
                } else if method_str == "net.peer_stats_export_all" {
                    #[cfg(feature = "telemetry")]
                    diagnostics::log::info!("peer_stats_export_all operator={peer_ip:?}");
                } else if method_str == "net.peer_throttle" {
                    #[cfg(feature = "telemetry")]
                    {
                        let clear = request
                            .params
                            .get("clear")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        diagnostics::log::info!("peer_throttle operator={peer_ip:?} clear={clear}");
                    }
                } else if method_str == "net.backpressure_clear" {
                    #[cfg(feature = "telemetry")]
                    diagnostics::log::info!("backpressure_clear operator={peer_ip:?}");
                }
                dispatch_result()
            }
        } else {
            dispatch_result()
        }
    } else {
        Err(RpcError {
            code: -32601,
            message: "method not found",
        })
    };

    match response {
        Ok(value) => RpcResponse::Result {
            jsonrpc: "2.0",
            result: value,
            id,
        },
        Err(error) => RpcResponse::Error {
            jsonrpc: "2.0",
            error,
            id,
        },
    }
}

async fn handle_rpc_options(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(response);
    }
    let origin = request.header("origin");
    let runtime_cfg = state.runtime();
    let allowed_origin = origin.and_then(|candidate| {
        if runtime_cfg
            .cors_allow_origins
            .iter()
            .any(|allowed| allowed == candidate)
        {
            Some(candidate)
        } else {
            None
        }
    });
    if let Some(origin) = allowed_origin {
        let response = Response::new(StatusCode::NO_CONTENT)
            .with_header("access-control-allow-methods", "POST")
            .with_header(
                "access-control-allow-headers",
                "content-type, authorization",
            );
        Ok(state.apply_cors(response, Some(origin)))
    } else {
        Ok(Response::new(StatusCode::FORBIDDEN).close())
    }
}

async fn handle_rpc_post(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    let remote = request.remote_addr();
    if let Some(mut response) = state.check_rate_limit(remote) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let auth = request.header("authorization");
    let peer_ip = Some(remote.ip());
    let origin = request.header("origin");
    let rpc_request =
        match foundation_serialization::json::from_slice::<RpcRequest>(request.body_bytes()) {
            Ok(req) => req,
            Err(_) => {
                let response = RpcResponse::Error {
                    jsonrpc: "2.0",
                    error: RpcError {
                        code: -32700,
                        message: "parse error",
                    },
                    id: None,
                };
                let response = Response::new(StatusCode::OK).json(&response)?;
                return Ok(state.apply_cors(response, origin));
            }
        };
    let rpc_response = execute_rpc(&state, rpc_request, auth, peer_ip);
    let response = Response::new(StatusCode::OK).json(&rpc_response)?;
    Ok(state.apply_cors(response, origin))
}

async fn handle_logs_search(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let path = render_request_path(&request);
    let (status, body) = logs::search_response(&path);
    let response = Response::new(status)
        .with_header("content-type", "application/json")
        .with_body(body.into_bytes());
    Ok(state.apply_cors(response, request.header("origin")))
}

async fn handle_logs_tail(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    let path = render_request_path(&request);
    match logs::build_tail_config(&path) {
        Ok(cfg) => Ok(WebSocketResponse::accept(move |stream| {
            let permit = permit;
            async move {
                logs::run_tail(stream, cfg).await;
                drop(permit);
                Ok(())
            }
        })),
        Err(err) => {
            let (status, body) = logs::map_search_error(err);
            let response = Response::new(status)
                .with_header("content-type", "application/json")
                .with_body(body.into_bytes())
                .close();
            Ok(WebSocketResponse::Reject(response))
        }
    }
}

async fn handle_vm_trace(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    if !crate::vm::vm_debug_enabled() {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    let code = request
        .query_param("code")
        .and_then(|hex| hex::decode(hex).ok())
        .unwrap_or_default();
    #[cfg(feature = "telemetry")]
    crate::telemetry::VM_TRACE_TOTAL.inc();
    Ok(WebSocketResponse::accept(move |stream| {
        let permit = permit;
        let code = code.clone();
        async move {
            vm_trace::run_trace(stream, code).await;
            drop(permit);
            Ok(())
        }
    }))
}

async fn handle_state_stream(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    #[cfg(feature = "telemetry")]
    crate::telemetry::STATE_STREAM_SUBSCRIBERS_TOTAL.inc();
    let bc = Arc::clone(&state.bc);
    Ok(WebSocketResponse::accept(move |stream| {
        let permit = permit;
        async move {
            state_stream::run_stream(stream, bc).await;
            drop(permit);
            Ok(())
        }
    }))
}

async fn handle_badge_status(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let (active, last_mint, last_burn) = {
        let mut chain = state.bc.lock().unwrap_or_else(|e| e.into_inner());
        chain.check_badges();
        chain.badge_status()
    };
    let body = foundation_serialization::json::json!({
        "active": active,
        "last_mint": last_mint,
        "last_burn": last_burn,
    })
    .to_string();
    let response = Response::new(StatusCode::OK)
        .with_header("content-type", "application/json")
        .with_body(body.into_bytes());
    Ok(state.apply_cors(response, request.header("origin")))
}

async fn handle_dashboard(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let body = include_str!("../../../dashboard/index.html")
        .as_bytes()
        .to_vec();
    let response = Response::new(StatusCode::OK)
        .with_header("content-type", "text/html; charset=utf-8")
        .with_body(body);
    Ok(state.apply_cors(response, request.header("origin")))
}

fn dispatch(
    req: &RpcRequest,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
) -> Result<foundation_serialization::json::Value, RpcError> {
    if BADGE_METHODS.contains(&req.method.as_str()) {
        let badge = req
            .params
            .get("badge")
            .and_then(|v| v.as_str())
            .or(req.badge.as_deref())
            .ok_or(RpcError {
                code: -32602,
                message: "badge required",
            })?;
        if !crate::service_badge::verify(badge) {
            return Err(RpcError {
                code: -32602,
                message: "invalid badge",
            });
        }
    }

    Ok(match req.method.as_str() {
        "set_difficulty" => {
            let val = req
                .params
                .get("value")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            match bc.lock() {
                Ok(mut guard) => {
                    guard.difficulty = val;
                    foundation_serialization::json::json!({"status": "ok"})
                }
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "balance" => {
            let addr = req
                .params
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(acct) = guard.accounts.get(addr) {
                foundation_serialization::json::json!({
                    "consumer": acct.balance.consumer,
                    "industrial": acct.balance.industrial,
                })
            } else {
                foundation_serialization::json::json!({"consumer": 0, "industrial": 0})
            }
        }
        "ledger.shard_of" => {
            let addr = req
                .params
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let shard = ledger::shard_of(addr);
            foundation_serialization::json::json!({"shard": shard})
        }
        "anomaly.label" => {
            let _label = req
                .params
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            #[cfg(feature = "telemetry")]
            crate::telemetry::ANOMALY_LABEL_TOTAL.inc();
            foundation_serialization::json::json!({"status": "ok"})
        }
        "settlement_status" => {
            let provider = req.params.get("provider").and_then(|v| v.as_str());
            let mode = match Settlement::mode() {
                SettleMode::DryRun => "dryrun",
                SettleMode::Real => "real",
                SettleMode::Armed { .. } => "armed",
            };
            if let Some(p) = provider {
                let (ct, industrial) = Settlement::balance_split(p);
                foundation_serialization::json::json!({"mode": mode, "balance": ct, "ct": ct, "industrial": industrial})
            } else {
                foundation_serialization::json::json!({"mode": mode})
            }
        }
        "settlement.audit" => compute_market::settlement_audit(),
        "bridge.relayer_status" => {
            let relayer = req
                .params
                .get("relayer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let asset = req.params.get("asset").and_then(|v| v.as_str());
            bridge::relayer_status(asset, relayer)
        }
        "bridge.bond_relayer" => {
            let relayer = req
                .params
                .get("relayer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let amount = req
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            bridge::bond_relayer(relayer, amount)?
        }
        "bridge.verify_deposit" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            let relayer = req
                .params
                .get("relayer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let user = req
                .params
                .get("user")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let amount = req
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let header = req
                .params
                .get("header")
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })
                .and_then(|v| {
                    foundation_serialization::json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            let proof = req
                .params
                .get("proof")
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })
                .and_then(|v| {
                    foundation_serialization::json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            let proofs = req
                .params
                .get("relayer_proofs")
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })
                .and_then(|v| {
                    foundation_serialization::json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            bridge::verify_deposit(asset, relayer, user, amount, header, proof, proofs)?
        }
        "bridge.request_withdrawal" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            let relayer = req
                .params
                .get("relayer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let user = req
                .params
                .get("user")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let amount = req
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let proofs = req
                .params
                .get("relayer_proofs")
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })
                .and_then(|v| {
                    foundation_serialization::json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            bridge::request_withdrawal(asset, relayer, user, amount, proofs)?
        }
        "bridge.challenge_withdrawal" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            let commitment = req
                .params
                .get("commitment")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let challenger = req
                .params
                .get("challenger")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            bridge::challenge_withdrawal(asset, commitment, challenger)?
        }
        "bridge.finalize_withdrawal" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            let commitment = req
                .params
                .get("commitment")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            bridge::finalize_withdrawal(asset, commitment)?
        }
        "bridge.pending_withdrawals" => {
            let asset = req.params.get("asset").and_then(|v| v.as_str());
            bridge::pending_withdrawals(asset)?
        }
        "bridge.active_challenges" => {
            let asset = req.params.get("asset").and_then(|v| v.as_str());
            bridge::active_challenges(asset)?
        }
        "bridge.relayer_quorum" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            bridge::relayer_quorum(asset)?
        }
        "bridge.deposit_history" => {
            let asset = req
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            let cursor = req.params.get("cursor").and_then(|v| v.as_u64());
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as usize;
            bridge::deposit_history(asset, cursor, limit)?
        }
        "bridge.slash_log" => bridge::slash_log()?,
        "localnet.submit_receipt" => {
            let hex = req
                .params
                .get("receipt")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = match hex::decode(hex) {
                Ok(b) => b,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let receipt: AssistReceipt = match binary::decode(&bytes) {
                Ok(r) => r,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            if !receipt.verify()
                || !validate_proximity(receipt.device, receipt.rssi, receipt.rtt_ms)
            {
                return Err(RpcError {
                    code: -32002,
                    message: "invalid receipt",
                });
            }
            let hash = receipt.hash();
            let key = format!("localnet_receipts/{}", hash);
            let mut db = LOCALNET_RECEIPTS.lock().unwrap_or_else(|e| e.into_inner());
            if db.get(&key).is_some() {
                foundation_serialization::json::json!({"status":"ignored"})
            } else {
                db.insert(&key, Vec::new());
                foundation_serialization::json::json!({"status":"ok"})
            }
        }
        "dns.publish_record" => match gateway::dns::publish_record(&req.params) {
            Ok(v) => v,
            Err(e) => {
                return Err(RpcError {
                    code: e.code(),
                    message: e.message(),
                })
            }
        },
        "gateway.policy" => gateway::dns::gateway_policy(&req.params),
        "gateway.reads_since" => gateway::dns::reads_since(&req.params),
        "gateway.dns_lookup" => gateway::dns::dns_lookup(&req.params),
        "gateway.mobile_cache_status" => gateway::mobile_cache::status_snapshot(),
        "gateway.mobile_cache_flush" => gateway::mobile_cache::flush_cache(),
        #[cfg(feature = "telemetry")]
        "telemetry.configure" => {
            #[derive(Deserialize)]
            struct TelemetryConfigure {
                sample_rate: Option<f64>,
                compaction_secs: Option<u64>,
            }
            let cfg: TelemetryConfigure = foundation_serialization::json::from_value(
                req.params.clone(),
            )
            .unwrap_or(TelemetryConfigure {
                sample_rate: None,
                compaction_secs: None,
            });
            if let Some(rate) = cfg.sample_rate {
                crate::telemetry::set_sample_rate(rate);
            }
            if let Some(secs) = cfg.compaction_secs {
                crate::telemetry::set_compaction_interval(secs);
            }
            foundation_serialization::json::json!({
                "status": "ok",
                "sample_rate_ppm": crate::telemetry::sample_rate_ppm(),
                "compaction_secs": crate::telemetry::compaction_interval_secs(),
            })
        }
        #[cfg(not(feature = "telemetry"))]
        "telemetry.configure" => {
            return Err(RpcError {
                code: -32603,
                message: "telemetry disabled",
            });
        }
        #[cfg(feature = "telemetry")]
        "analytics" => {
            let q: analytics::AnalyticsQuery = foundation_serialization::json::from_value(
                req.params.clone(),
            )
            .unwrap_or(analytics::AnalyticsQuery {
                domain: String::new(),
            });
            let stats = analytics::analytics(&crate::telemetry::READ_STATS, q);
            foundation_serialization::json::to_value(stats).unwrap()
        }
        "microshard.roots.last" => {
            let n = req.params.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            compute_market::recent_roots(n)
        }
        "mempool.stats" => {
            let lane_str = req
                .params
                .get("lane")
                .and_then(|v| v.as_str())
                .unwrap_or("consumer");
            let lane = match lane_str {
                "industrial" => FeeLane::Industrial,
                _ => FeeLane::Consumer,
            };
            let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
            let stats = guard.mempool_stats(lane);
            foundation_serialization::json::json!({
                "size": stats.size,
                "age_p50": stats.age_p50,
                "age_p95": stats.age_p95,
                "fee_p50": stats.fee_p50,
                "fee_p90": stats.fee_p90,
                "fee_floor": stats.fee_floor,
            })
        }
        "mempool.qos_event" => {
            let lane = req
                .params
                .get("lane")
                .and_then(|v| v.as_str())
                .unwrap_or("consumer");
            let event = req
                .params
                .get("event")
                .and_then(|v| v.as_str())
                .unwrap_or("warning");
            let fee = req.params.get("fee").and_then(|v| v.as_u64()).unwrap_or(0);
            let floor = req
                .params
                .get("floor")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            #[cfg(feature = "telemetry")]
            {
                use crate::telemetry::{FEE_FLOOR_OVERRIDE_TOTAL, FEE_FLOOR_WARNING_TOTAL};
                let labels = [lane];
                FEE_FLOOR_WARNING_TOTAL.with_label_values(&labels).inc();
                if event == "override" {
                    FEE_FLOOR_OVERRIDE_TOTAL.with_label_values(&labels).inc();
                }
                diagnostics::tracing::info!(
                    target: "mempool",
                    lane,
                    event,
                    fee,
                    floor,
                    "wallet fee floor event",
                );
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = (lane, event, fee, floor);
            }
            foundation_serialization::json::json!({"status": "ok"})
        }
        "net.overlay_status" => {
            let status = net::overlay_status();
            foundation_serialization::json::json!({
                "backend": status.backend,
                "active_peers": status.active_peers,
                "persisted_peers": status.persisted_peers,
                "database_path": status.database_path,
            })
        }
        "net.peer_stats" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            let m = net::peer_stats(&pk).ok_or(RpcError {
                code: -32602,
                message: "unknown peer",
            })?;
            foundation_serialization::json::json!({
                "requests": m.requests,
                "bytes_sent": m.bytes_sent,
                "drops": m.drops,
                "handshake_fail": m.handshake_fail,
                "reputation": m.reputation.score,
                "throttle_reason": m.throttle_reason,
                "throttled_until": m.throttled_until,
            })
        }
        "net.peer_stats_all" => {
            let offset = req
                .params
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as usize;
            let stats = net::peer_stats_all(offset, limit);
            foundation_serialization::json::to_value(stats).unwrap()
        }
        "net.peer_stats_reset" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            if net::reset_peer_metrics(&pk) {
                foundation_serialization::json::json!({"status": "ok"})
            } else {
                return Err(RpcError {
                    code: -32602,
                    message: "unknown peer",
                });
            }
        }
        "net.peer_stats_export" => {
            let path = req
                .params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let all = req
                .params
                .get("all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let min_rep = req.params.get("min_reputation").and_then(|v| v.as_f64());
            let active = req.params.get("active_within").and_then(|v| v.as_u64());
            if all {
                match net::export_all_peer_stats(path, min_rep, active) {
                    Ok(over) => {
                        foundation_serialization::json::json!({"status": "ok", "overwritten": over})
                    }
                    Err(e) => {
                        return Err(RpcError {
                            code: -32602,
                            message: io_err_msg(&e),
                        });
                    }
                }
            } else {
                let id = req
                    .params
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pk = parse_overlay_peer_param(id)?;
                match net::export_peer_stats(&pk, path) {
                    Ok(over) => {
                        foundation_serialization::json::json!({"status": "ok", "overwritten": over})
                    }
                    Err(e) => {
                        return Err(RpcError {
                            code: -32602,
                            message: io_err_msg(&e),
                        });
                    }
                }
            }
        }
        "net.peer_stats_export_all" => {
            let min_rep = req.params.get("min_reputation").and_then(|v| v.as_f64());
            let active = req.params.get("active_within").and_then(|v| v.as_u64());
            let map = net::peer_stats_map(min_rep, active);
            foundation_serialization::json::to_value(map).unwrap()
        }
        "net.peer_stats_persist" => match net::persist_peer_metrics() {
            Ok(()) => foundation_serialization::json::json!({"status": "ok"}),
            Err(_) => {
                return Err(RpcError {
                    code: -32603,
                    message: "persist failed",
                });
            }
        },
        "net.peer_throttle" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let clear = req
                .params
                .get("clear")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pk = parse_overlay_peer_param(id)?;
            if clear {
                if net::clear_throttle(&pk) {
                    foundation_serialization::json::json!({"status": "ok"})
                } else {
                    return Err(RpcError {
                        code: -32602,
                        message: "unknown peer",
                    });
                }
            } else {
                net::throttle_peer(&pk, "manual");
                foundation_serialization::json::json!({"status": "ok"})
            }
        }
        "net.backpressure_clear" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            if net::clear_throttle(&pk) {
                foundation_serialization::json::json!({"status": "ok"})
            } else {
                return Err(RpcError {
                    code: -32602,
                    message: "unknown peer",
                });
            }
        }
        "net.reputation_sync" => {
            net::reputation_sync();
            foundation_serialization::json::json!({"status": "ok"})
        }
        "net.rotate_cert" => {
            #[cfg(feature = "quic")]
            {
                let key = crate::net::load_net_key();
                match crate::net::transport_quic::rotate(&key) {
                    Ok(advert) => {
                        let previous: Vec<String> =
                            advert.previous.iter().map(|fp| hex::encode(fp)).collect();
                        foundation_serialization::json::json!({
                            "status": "ok",
                            "fingerprint": hex::encode(advert.fingerprint),
                            "previous": previous,
                        })
                    }
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        diagnostics::tracing::error!(error = %err, "quic_cert_rotation_failed");
                        #[cfg(not(feature = "telemetry"))]
                        let _ = err;
                        return Err(RpcError {
                            code: -32603,
                            message: "rotation failed",
                        });
                    }
                }
            }
            #[cfg(not(feature = "quic"))]
            {
                return Err(RpcError {
                    code: -32601,
                    message: "quic feature not enabled",
                });
            }
        }
        "net.key_rotate" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            let new_key = req
                .params
                .get("new_key")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            let sig_hex = req
                .params
                .get("signature")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            let old_bytes = hex::decode(id).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let new_bytes = hex::decode(new_key).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let sig_bytes = hex::decode(sig_hex).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let old_pk: [u8; 32] = old_bytes.try_into().map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let new_pk: [u8; 32] = new_bytes.try_into().map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let sig = Signature::from_bytes(&sig_arr);
            let vk = VerifyingKey::from_bytes(&old_pk).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            if vk.verify(&new_pk, &sig).is_err() {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .with_label_values(&["bad_sig"])
                    .inc();
                return Err(RpcError {
                    code: -32602,
                    message: "bad signature",
                });
            }
            if net::rotate_peer_key(&old_pk, new_pk) {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .with_label_values(&["ok"])
                    .inc();
                foundation_serialization::json::json!({"status":"ok"})
            } else {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .with_label_values(&["missing"])
                    .inc();
                return Err(RpcError {
                    code: -32602,
                    message: "unknown peer",
                });
            }
        }
        "net.handshake_failures" => {
            let entries = net::recent_handshake_failures();
            foundation_serialization::json::json!({"failures": entries})
        }
        "net.quic_stats" => match foundation_serialization::json::to_value(net::quic_stats()) {
            Ok(val) => val,
            Err(e) => {
                #[cfg(feature = "telemetry")]
                diagnostics::tracing::warn!(target: "rpc", error = %e, "failed to serialize quic stats");
                #[cfg(not(feature = "telemetry"))]
                let _ = e;
                return Err(RpcError {
                    code: -32603,
                    message: "serialization error",
                });
            }
        },
        "net.quic_certs" => {
            match foundation_serialization::json::to_value(net::peer_cert_history()) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(target: "rpc", error = %e, "failed to serialize quic cert history");
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(RpcError {
                        code: -32603,
                        message: "serialization error",
                    });
                }
            }
        }
        "net.quic_certs_refresh" => {
            let refreshed = net::refresh_peer_cert_store_from_disk();
            foundation_serialization::json::json!({ "reloaded": refreshed })
        }
        "peer.rebate_status" => {
            let peer = req
                .params
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let threshold = req
                .params
                .get("threshold")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pk = net::uptime::peer_from_bytes(&hex::decode(peer).unwrap_or_default()).map_err(
                |_| RpcError {
                    code: -32602,
                    message: "bad peer",
                },
            )?;
            let eligible = net::uptime::eligible(&pk, threshold, epoch);
            foundation_serialization::json::json!({"eligible": eligible})
        }
        "peer.rebate_claim" => {
            let peer = req
                .params
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let threshold = req
                .params
                .get("threshold")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let reward = req
                .params
                .get("reward")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pk = net::uptime::peer_from_bytes(&hex::decode(peer).unwrap_or_default()).map_err(
                |_| RpcError {
                    code: -32602,
                    message: "bad peer",
                },
            )?;
            let voucher = net::uptime::claim(pk, threshold, epoch, reward).unwrap_or(0);
            foundation_serialization::json::json!({"voucher": voucher})
        }
        "net.config_reload" => {
            if crate::config::reload() {
                foundation_serialization::json::json!({"status": "ok"})
            } else {
                return Err(RpcError {
                    code: -32603,
                    message: "reload failed",
                });
            }
        }
        "kyc.verify" => {
            let user = req
                .params
                .get("user")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match kyc::verify(user) {
                Ok(true) => foundation_serialization::json::json!({"status": "verified"}),
                Ok(false) => foundation_serialization::json::json!({"status": "denied"}),
                Err(_) => {
                    return Err(RpcError {
                        code: -32080,
                        message: "kyc failure",
                    });
                }
            }
        }
        "pow.get_template" => {
            // simplistic template: zero prev/merkle
            let tmpl = pow::template([0u8; 32], [0u8; 32], [0u8; 32], 1_000_000, 1, 0);
            foundation_serialization::json::json!({
                "prev_hash": hex::encode(tmpl.prev_hash),
                "merkle_root": hex::encode(tmpl.merkle_root),
                "checkpoint_hash": hex::encode(tmpl.checkpoint_hash),
                "difficulty": tmpl.difficulty,
                "base_fee": tmpl.base_fee,
                "timestamp_millis": tmpl.timestamp_millis
            })
        }
        "pow.submit" => {
            let header_obj = req.params.get("header").ok_or(RpcError {
                code: -32602,
                message: "missing header",
            })?;
            let parse32 =
                |v: &foundation_serialization::json::Value| -> Result<[u8; 32], RpcError> {
                    let s = v.as_str().ok_or(RpcError {
                        code: -32602,
                        message: "bad hex",
                    })?;
                    let bytes = hex::decode(s).map_err(|_| RpcError {
                        code: -32602,
                        message: "bad hex",
                    })?;
                    let arr: [u8; 32] = bytes.try_into().map_err(|_| RpcError {
                        code: -32602,
                        message: "bad hex",
                    })?;
                    Ok(arr)
                };
            let prev_hash = parse32(&header_obj["prev_hash"])?;
            let merkle_root = parse32(&header_obj["merkle_root"])?;
            let checkpoint_hash = parse32(&header_obj["checkpoint_hash"])?;
            let nonce = header_obj["nonce"].as_u64().ok_or(RpcError {
                code: -32602,
                message: "bad nonce",
            })?;
            let difficulty = header_obj["difficulty"].as_u64().ok_or(RpcError {
                code: -32602,
                message: "bad difficulty",
            })?;
            let timestamp = header_obj["timestamp_millis"].as_u64().ok_or(RpcError {
                code: -32602,
                message: "bad timestamp_millis",
            })?;
            let retune_hint = header_obj["retune_hint"].as_i64().unwrap_or(0) as i8;
            let hdr = BlockHeader {
                prev_hash,
                merkle_root,
                checkpoint_hash,
                nonce,
                difficulty,
                retune_hint,
                base_fee: header_obj["base_fee"].as_u64().unwrap_or(1),
                timestamp_millis: timestamp,
                l2_roots: Vec::new(),
                l2_sizes: Vec::new(),
                vdf_commit: [0u8; 32],
                vdf_output: [0u8; 32],
                vdf_proof: Vec::new(),
            };
            let hash = hdr.hash();
            let val = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
            if val <= u64::MAX / difficulty.max(1) {
                foundation_serialization::json::json!({"status":"accepted"})
            } else {
                return Err(RpcError {
                    code: -32082,
                    message: "invalid pow",
                });
            }
        }
        "consensus.difficulty" => consensus::difficulty(&bc),
        "consensus.pos.register" => pos::register(&req.params)?,
        "consensus.pos.bond" => pos::bond(&req.params)?,
        "consensus.pos.unbond" => pos::unbond(&req.params)?,
        "consensus.pos.slash" => pos::slash(&req.params)?,
        "light.latest_header" => {
            let guard = bc.lock().unwrap();
            foundation_serialization::json::to_value(light::latest_header(&guard)).unwrap()
        }
        "light.headers" => {
            let start = req
                .params
                .get("start")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(200) as usize;
            let guard = bc.lock().unwrap();
            foundation_serialization::json::to_value(light::headers_since(&guard, start, limit))
                .unwrap()
        }
        "light_client.rebate_status" => {
            let status = {
                let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
                light::rebate_status(&guard)
            };
            match foundation_serialization::json::to_value(status) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(
                        target: "rpc",
                        error = %e,
                        "failed to serialize rebate status"
                    );
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(RpcError {
                        code: -32603,
                        message: "serialization error".into(),
                    });
                }
            }
        }
        "light_client.rebate_history" => {
            let relayer = if let Some(hex) = req.params.get("relayer").and_then(|v| v.as_str()) {
                match hex::decode(hex) {
                    Ok(bytes) => Some(bytes),
                    Err(_) => {
                        return Err(RpcError {
                            code: -32602,
                            message: "invalid relayer id".into(),
                        });
                    }
                }
            } else {
                None
            };
            let cursor = req.params.get("cursor").and_then(|v| v.as_u64());
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(25)
                .min(200) as usize;
            let history = {
                let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
                light::rebate_history(&guard, relayer.as_deref(), cursor, limit)
            };
            match foundation_serialization::json::to_value(history) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(
                        target: "rpc",
                        error = %e,
                        "failed to serialize rebate history"
                    );
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(RpcError {
                        code: -32603,
                        message: "serialization error".into(),
                    });
                }
            }
        }
        "rent.escrow.balance" => {
            let esc = RentEscrow::open("rent_escrow.db");
            if let Some(id) = req.params.get("id").and_then(|v| v.as_str()) {
                foundation_serialization::json::json!({"balance": esc.balance(id)})
            } else if let Some(acct) = req.params.get("account").and_then(|v| v.as_str()) {
                foundation_serialization::json::json!({"balance": esc.balance_account(acct)})
            } else {
                foundation_serialization::json::json!({"balance": 0})
            }
        }
        "mesh.peers" => {
            foundation_serialization::json::json!({"peers": range_boost::peers()})
        }
        "inflation.params" => inflation::params(&bc),
        "compute_market.stats" => {
            let accel = req
                .params
                .get("accelerator")
                .and_then(|v| v.as_str())
                .and_then(|s| match s.to_lowercase().as_str() {
                    "fpga" => Some(crate::compute_market::Accelerator::Fpga),
                    "tpu" => Some(crate::compute_market::Accelerator::Tpu),
                    _ => None,
                });
            compute_market::stats(accel)
        }
        "compute_market.provider_balances" => compute_market::provider_balances(),
        "compute_market.audit" => compute_market::settlement_audit(),
        "compute_market.recent_roots" => {
            let n = req.params.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            compute_market::recent_roots(n)
        }
        "compute_market.scheduler_metrics" => compute_market::scheduler_metrics(),
        "compute_market.scheduler_stats" => compute_market::scheduler_stats(),
        "scheduler.stats" => scheduler::stats(),
        "compute.job_status" => {
            let job_id = req
                .params
                .get("job_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            crate::compute_market::job_status(job_id)
        }
        "compute.reputation_get" => {
            let provider = req
                .params
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            compute_market::reputation_get(provider)
        }
        "compute.job_requirements" => {
            let job_id = req
                .params
                .get("job_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            compute_market::job_requirements(job_id)
        }
        "compute.job_cancel" => {
            let job_id = req
                .params
                .get("job_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            compute_market::job_cancel(job_id)
        }
        "compute.provider_hardware" => {
            let provider = req
                .params
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            compute_market::provider_hardware(provider)
        }
        "net.reputation_show" => {
            let peer = req
                .params
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            net::reputation_show(peer)
        }
        "net.gossip_status" => {
            if let Some(status) = net::gossip_status() {
                foundation_serialization::json::to_value(status)
                    .unwrap_or_else(|_| foundation_serialization::json::json!({}))
            } else {
                foundation_serialization::json::json!({"status": "unavailable"})
            }
        }
        "net.dns_verify" => {
            let domain = req
                .params
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            net::dns_verify(domain)
        }
        "stake.role" => pos::role(&req.params)?,
        "config.reload" => {
            let ok = crate::config::reload();
            foundation_serialization::json::json!({"reloaded": ok})
        }
        "register_handle" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            match handles.lock() {
                Ok(mut reg) => match identity::register_handle(&req.params, &mut reg) {
                    Ok(v) => v,
                    Err(e) => foundation_serialization::json::json!({"error": e.code()}),
                },
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "identity.anchor" => {
            let scope = req
                .params
                .get("address")
                .and_then(|v| v.as_str())
                .map(|addr| format!("{}:{}", req.method, addr))
                .unwrap_or_else(|| req.method.clone());
            check_nonce(scope, &req.params, &nonces)?;
            match dids.lock() {
                Ok(mut reg) => match identity::anchor_did(&req.params, &mut reg, &GOV_STORE) {
                    Ok(v) => v,
                    Err(e) => foundation_serialization::json::json!({"error": e.code()}),
                },
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "resolve_handle" => match handles.lock() {
            Ok(reg) => identity::resolve_handle(&req.params, &reg),
            Err(_) => foundation_serialization::json::json!({"address": null}),
        },
        "identity.resolve" => match dids.lock() {
            Ok(reg) => identity::resolve_did(&req.params, &reg),
            Err(_) => foundation_serialization::json::json!({
                "address": foundation_serialization::json::Value::Null,
                "document": foundation_serialization::json::Value::Null,
                "hash": foundation_serialization::json::Value::Null,
                "nonce": foundation_serialization::json::Value::Null,
                "updated_at": foundation_serialization::json::Value::Null,
            }),
        },
        "whoami" => match handles.lock() {
            Ok(reg) => identity::whoami(&req.params, &reg),
            Err(_) => foundation_serialization::json::json!({"address": null, "handle": null}),
        },
        "record_le_request" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let agency = req
                .params
                .get("agency")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let case = req
                .params
                .get("case")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    let jurisdiction = guard.config.jurisdiction.as_deref().unwrap_or("UNSPEC");
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_request(&base, agency, case, jurisdiction, lang)
                    {
                        Ok(_) => foundation_serialization::json::json!({"status": "ok"}),
                        Err(_) => foundation_serialization::json::json!({"error": "io"}),
                    }
                }
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "warrant_canary" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let msg = req
                .params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    match crate::le_portal::record_canary(&base, msg) {
                        Ok(hash) => foundation_serialization::json::json!({"hash": hash}),
                        Err(_) => foundation_serialization::json::json!({"error": "io"}),
                    }
                }
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "le.list_requests" => match bc.lock() {
            Ok(guard) => {
                let base = guard.path.clone();
                match crate::le_portal::list_requests(&base) {
                    Ok(v) => foundation_serialization::json::to_value(v).unwrap_or_default(),
                    Err(_) => foundation_serialization::json::json!({"error": "io"}),
                }
            }
            Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
        },
        "le.record_action" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let agency = req
                .params
                .get("agency")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let action = req
                .params
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    let jurisdiction = guard.config.jurisdiction.as_deref().unwrap_or("UNSPEC");
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_action(&base, agency, action, jurisdiction, lang)
                    {
                        Ok(hash) => foundation_serialization::json::json!({"hash": hash}),
                        Err(_) => foundation_serialization::json::json!({"error": "io"}),
                    }
                }
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "le.upload_evidence" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let agency = req
                .params
                .get("agency")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let case_id = req
                .params
                .get("case_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let data_b64 = req
                .params
                .get("evidence")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let data = match decode_standard(data_b64) {
                Ok(d) => d,
                Err(_) => return Ok(foundation_serialization::json::json!({"error": "decode"})),
            };
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    let jurisdiction = guard.config.jurisdiction.as_deref().unwrap_or("UNSPEC");
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_evidence(
                        &base,
                        agency,
                        case_id,
                        jurisdiction,
                        lang,
                        &data,
                    ) {
                        Ok(hash) => foundation_serialization::json::json!({"hash": hash}),
                        Err(_) => foundation_serialization::json::json!({"error": "io"}),
                    }
                }
                Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
            }
        }
        "service_badge_issue" => match bc.lock() {
            Ok(mut guard) => {
                let token = guard.badge_tracker_mut().force_issue();
                foundation_serialization::json::json!({"badge": token})
            }
            Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
        },
        "service_badge_revoke" => match bc.lock() {
            Ok(mut guard) => {
                guard.badge_tracker_mut().revoke();
                foundation_serialization::json::json!({"revoked": true})
            }
            Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
        },
        "service_badge_verify" => {
            let badge = req
                .params
                .get("badge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            foundation_serialization::json::json!({"valid": crate::service_badge::verify(badge)})
        }
        "submit_tx" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let tx_hex = req.params.get("tx").and_then(|v| v.as_str()).unwrap_or("");
            match hex::decode(tx_hex)
                .ok()
                .and_then(|b| binary::decode::<SignedTransaction>(&b).ok())
            {
                Some(mut tx) => {
                    if let Some(f) = req.params.get("max_fee").and_then(|v| v.as_u64()) {
                        tx.payload.fee = f;
                    }
                    if let Some(t) = req.params.get("tip").and_then(|v| v.as_u64()) {
                        tx.tip = t;
                    }
                    match bc.lock() {
                        Ok(mut guard) => match guard.submit_transaction(tx) {
                            Ok(()) => foundation_serialization::json::json!({"status": "ok"}),
                            Err(e) => {
                                foundation_serialization::json::json!({"error": format!("{e:?}")})
                            }
                        },
                        Err(_) => foundation_serialization::json::json!({"error": "lock poisoned"}),
                    }
                }
                None => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            }
        }
        "set_snapshot_interval" => {
            let interval = req
                .params
                .get("interval")
                .and_then(|v| v.as_u64())
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })?;
            if interval < 10 {
                return Err(SnapshotError::IntervalTooSmall.into());
            }
            if let Ok(mut guard) = bc.lock() {
                guard.snapshot.set_interval(interval);
                guard.config.snapshot_interval = interval;
                guard.save_config();
            } else {
                return Err(RpcError {
                    code: -32603,
                    message: "lock poisoned",
                });
            }
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::SNAPSHOT_INTERVAL.set(interval as i64);
                crate::telemetry::SNAPSHOT_INTERVAL_CHANGED.set(interval as i64);
            }
            #[cfg(feature = "telemetry")]
            diagnostics::log::info!("snapshot_interval_changed {interval}");
            foundation_serialization::json::json!({"status": "ok"})
        }
        "start_mining" => {
            if runtime_cfg.relay_only {
                foundation_serialization::json::json!({
                    "error": {"code": -32075, "message": "relay_only"}
                })
            } else {
                check_nonce(req.method.as_str(), &req.params, &nonces)?;
                let miner = req
                    .params
                    .get("miner")
                    .and_then(|v| v.as_str())
                    .unwrap_or("miner");
                if !mining.swap(true, Ordering::SeqCst) {
                    let bc = Arc::clone(&bc);
                    let miner = miner.to_string();
                    let flag = Arc::clone(&mining);
                    std::thread::spawn(move || {
                        while flag.load(Ordering::SeqCst) {
                            if let Ok(mut g) = bc.lock() {
                                let _ = g.mine_block(&miner);
                            }
                        }
                    });
                }
                foundation_serialization::json::json!({"status": "ok"})
            }
        }
        "stop_mining" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            mining.store(false, Ordering::SeqCst);
            foundation_serialization::json::json!({"status": "ok"})
        }
        "jurisdiction.status" => jurisdiction::status(&bc)?,
        "jurisdiction.set" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let path = req
                .params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32072,
                    message: "missing path",
                })?;
            jurisdiction::set(&bc, path)?
        }
        "jurisdiction.policy_diff" => {
            let path = req
                .params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or(RpcError {
                    code: -32072,
                    message: "missing path",
                })?;
            jurisdiction::policy_diff(&bc, path)?
        }
        "metrics" => {
            #[cfg(feature = "telemetry")]
            {
                let m = crate::gather_metrics().unwrap_or_default();
                foundation_serialization::json::json!(m)
            }
            #[cfg(not(feature = "telemetry"))]
            {
                foundation_serialization::json::json!("telemetry disabled")
            }
        }
        "price_board_get" => {
            let lane = req
                .params
                .get("lane")
                .and_then(|v| v.as_str())
                .unwrap_or("consumer");
            let lane = if lane == "industrial" {
                FeeLane::Industrial
            } else {
                FeeLane::Consumer
            };
            match crate::compute_market::price_board::bands(lane) {
                Some((p25, median, p75)) => {
                    foundation_serialization::json::json!({"p25": p25, "median": median, "p75": p75})
                }
                None => {
                    return Err(crate::compute_market::MarketError::NoPriceData.into());
                }
            }
        }
        "compute_arm_real" => {
            let delay = req
                .params
                .get("activate_in_blocks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let height = bc.lock().unwrap_or_else(|e| e.into_inner()).block_height;
            crate::compute_market::settlement::Settlement::arm(delay, height);
            foundation_serialization::json::json!({"status": "ok"})
        }
        "compute_cancel_arm" => {
            crate::compute_market::settlement::Settlement::cancel_arm();
            foundation_serialization::json::json!({"status": "ok"})
        }
        "compute_back_to_dry_run" => {
            let reason = req
                .params
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            crate::compute_market::settlement::Settlement::back_to_dry_run(reason);
            foundation_serialization::json::json!({"status": "ok"})
        }
        "dex_escrow_status" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            dex::escrow_status(id)
        }
        "dex_escrow_release" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let amt = req
                .params
                .get("amount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            match dex::escrow_release(id, amt) {
                Ok(v) => v,
                Err(_) => {
                    return Err(RpcError {
                        code: -32002,
                        message: "release failed",
                    })
                }
            }
        }
        "dex_escrow_proof" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let idx = req
                .params
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            if let Some(proof) = dex::escrow_proof(id, idx) {
                foundation_serialization::json::to_value(proof).map_err(|_| RpcError {
                    code: -32603,
                    message: "internal error",
                })?
            } else {
                return Err(RpcError {
                    code: -32003,
                    message: "not found",
                });
            }
        }
        "htlc_status" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            htlc::status(id)
        }
        "htlc_refund" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let now = req.params.get("now").and_then(|v| v.as_u64()).unwrap_or(0);
            htlc::refund(id, now)
        }
        "storage_upload" => {
            let object_id = req
                .params
                .get("object_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let provider_id = req
                .params
                .get("provider_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let original_bytes = req
                .params
                .get("original_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let shares = req
                .params
                .get("shares")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
            let price_per_block = req
                .params
                .get("price_per_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let start_block = req
                .params
                .get("start_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let retention_blocks = req
                .params
                .get("retention_blocks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let contract = StorageContract {
                object_id: object_id.clone(),
                provider_id: provider_id.clone(),
                original_bytes,
                shares,
                price_per_block,
                start_block,
                retention_blocks,
                next_payment_block: start_block + 1,
                accrued: 0,
            };
            let offer = StorageOffer::new(
                provider_id,
                original_bytes,
                price_per_block,
                retention_blocks,
            );
            storage::upload(contract, vec![offer])
        }
        "storage_challenge" => {
            let object_id = req
                .params
                .get("object_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let chunk_idx = req
                .params
                .get("chunk_idx")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let proof = req
                .params
                .get("proof")
                .and_then(|v| v.as_str())
                .map(|s| {
                    let bytes = hex::decode(s).unwrap_or_default();
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes[..32.min(bytes.len())]);
                    arr
                })
                .unwrap_or([0u8; 32]);
            let current_block = req
                .params
                .get("current_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            storage::challenge(object_id, chunk_idx, proof, current_block)
        }
        "storage.manifests" => {
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            storage::manifest_summaries(limit)
        }
        "storage_provider_profiles" => storage::provider_profiles(),
        "storage_provider_set_maintenance" => {
            let provider = req
                .params
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let maintenance = req
                .params
                .get("maintenance")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            storage::set_provider_maintenance(provider, maintenance)
        }
        "gov_propose" => {
            let proposer = req
                .params
                .get("proposer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key = req.params.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let new_value = req
                .params
                .get("new_value")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let min = req.params.get("min").and_then(|v| v.as_i64()).unwrap_or(0);
            let max = req.params.get("max").and_then(|v| v.as_i64()).unwrap_or(0);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let deadline = req
                .params
                .get("vote_deadline")
                .and_then(|v| v.as_u64())
                .unwrap_or(epoch);
            governance::gov_propose(
                &GOV_STORE, proposer, key, new_value, min, max, epoch, deadline,
            )?
        }
        "gov_vote" => {
            let voter = req
                .params
                .get("voter")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let pid = req
                .params
                .get("proposal_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let choice = req
                .params
                .get("choice")
                .and_then(|v| v.as_str())
                .unwrap_or("yes");
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            governance::gov_vote(&GOV_STORE, voter, pid, choice, epoch)?
        }
        "submit_proposal" => {
            let proposer = req
                .params
                .get("proposer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key = req.params.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let new_value = req
                .params
                .get("new_value")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let min = req.params.get("min").and_then(|v| v.as_i64()).unwrap_or(0);
            let max = req.params.get("max").and_then(|v| v.as_i64()).unwrap_or(0);
            let deps = req
                .params
                .get("deps")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                .unwrap_or_else(|| vec![]);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let deadline = req
                .params
                .get("vote_deadline")
                .and_then(|v| v.as_u64())
                .unwrap_or(epoch);
            governance::submit_proposal(
                &GOV_STORE, proposer, key, new_value, min, max, deps, epoch, deadline,
            )?
        }
        "vote_proposal" => {
            let voter = req
                .params
                .get("voter")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let pid = req
                .params
                .get("proposal_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let choice = req
                .params
                .get("choice")
                .and_then(|v| v.as_str())
                .unwrap_or("yes");
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            governance::vote_proposal(&GOV_STORE, voter, pid, choice, epoch)?
        }
        "gov.release_signers" => governance::release_signers(&GOV_STORE)?,
        "gov_list" => governance::gov_list(&GOV_STORE)?,
        "gov_params" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let params = GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
            governance::gov_params(&params, epoch)?
        }
        "gov_rollback_last" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let mut params = GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
            let mut chain = bc.lock().unwrap_or_else(|e| e.into_inner());
            let mut rt = crate::governance::Runtime { bc: &mut *chain };
            governance::gov_rollback_last(&GOV_STORE, &mut params, &mut rt, epoch)?
        }
        "gov_rollback" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut params = GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
            let mut chain = bc.lock().unwrap_or_else(|e| e.into_inner());
            let mut rt = crate::governance::Runtime { bc: &mut *chain };
            governance::gov_rollback(&GOV_STORE, id, &mut params, &mut rt, epoch)?
        }
        "vm.estimate_gas" => {
            let code_hex = req
                .params
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = hex::decode(code_hex).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let gas = vm::estimate_gas(code);
            foundation_serialization::json::json!({"gas_used": gas})
        }
        "vm.exec_trace" => {
            let code_hex = req
                .params
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = hex::decode(code_hex).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            let trace = vm::exec_trace(code);
            foundation_serialization::json::json!({"trace": trace})
        }
        "vm.storage_read" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data = vm::storage_read(id).unwrap_or_default();
            foundation_serialization::json::json!({"data": hex::encode(data)})
        }
        "vm.storage_write" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data_hex = req
                .params
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = hex::decode(data_hex).map_err(|_| RpcError {
                code: -32602,
                message: "invalid params",
            })?;
            vm::storage_write(id, bytes);
            foundation_serialization::json::json!({"status": "ok"})
        }
        _ => {
            return Err(RpcError {
                code: -32601,
                message: "method not found",
            })
        }
    })
}

pub async fn run_rpc_server(
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    addr: String,
    cfg: RpcConfig,
    ready: oneshot::Sender<String>,
) -> std::io::Result<()> {
    let bind_addr: SocketAddr = addr.parse().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid rpc bind address {addr}: {err}"),
        )
    })?;
    let listener = TcpListener::bind(bind_addr).await?;
    let local = listener.local_addr()?.to_string();
    let _ = ready.send(local);

    let nonces = Arc::new(Mutex::new(HashSet::<(String, u64)>::new()));
    let handles = Arc::new(Mutex::new(HandleRegistry::open("identity_db")));
    let did_path = DidRegistry::default_path();
    let dids = Arc::new(Mutex::new(DidRegistry::open(&did_path)));
    let clients = Arc::new(Mutex::new(HashMap::<IpAddr, ClientState>::new()));
    let tokens_per_sec = std::env::var("TB_RPC_TOKENS_PER_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100.0);
    let ban_secs = std::env::var("TB_RPC_BAN_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let client_timeout = std::env::var("TB_RPC_CLIENT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);

    let admin_token = cfg
        .admin_token_file
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string());
    let runtime_cfg = Arc::new(RpcRuntimeConfig {
        allowed_hosts: cfg.allowed_hosts,
        cors_allow_origins: cfg.cors_allow_origins,
        max_body_bytes: cfg.max_body_bytes,
        request_timeout: Duration::from_millis(cfg.request_timeout_ms),
        enable_debug: cfg.enable_debug,
        admin_token,
        relay_only: cfg.relay_only,
    });

    let state = RpcState {
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg: Arc::clone(&runtime_cfg),
        clients,
        tokens_per_sec,
        ban_secs,
        client_timeout,
        concurrent: Arc::new(Semaphore::new(1024)),
    };

    let router = Router::new(state)
        .route(Method::Options, "/", handle_rpc_options)
        .route(Method::Post, "/", handle_rpc_post)
        .get("/logs/search", handle_logs_search)
        .upgrade("/logs/tail", handle_logs_tail)
        .upgrade("/vm/trace", handle_vm_trace)
        .upgrade("/state_stream", handle_state_stream)
        .get("/badge/status", handle_badge_status)
        .get("/dashboard", handle_dashboard);

    let mut server_cfg = ServerConfig::default();
    server_cfg.request_timeout = runtime_cfg.request_timeout;
    server_cfg.max_body_bytes = runtime_cfg.max_body_bytes;
    serve(listener, router, server_cfg).await
}

#[cfg(test)]
pub fn fuzz_runtime_config() -> Arc<RpcRuntimeConfig> {
    Arc::new(RpcRuntimeConfig {
        allowed_hosts: vec!["localhost".into()],
        cors_allow_origins: Vec::new(),
        max_body_bytes: 1024,
        request_timeout: Duration::from_secs(1),
        enable_debug: false,
        admin_token: None,
        relay_only: false,
    })
}
