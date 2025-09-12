use crate::{
    compute_market::settlement::{SettleMode, Settlement},
    config::RpcConfig,
    consensus::pow::{self, BlockHeader},
    gateway,
    governance::{GovStore, Params},
    identity::handle_registry::HandleRegistry,
    kyc,
    localnet::{validate_proximity, AssistReceipt},
    net,
    simple_db::SimpleDb,
    storage::fs::RentEscrow,
    transaction::FeeLane,
    Blockchain, SignedTransaction,
};
use bincode;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hex;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fs;
use std::net::IpAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

pub mod limiter;
use limiter::{ClientState, RpcClientErrorCode};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[cfg(feature = "telemetry")]
pub mod analytics;
pub mod bridge;
pub mod client;
pub mod compute_market;
pub mod consensus;
pub mod dex;
pub mod governance;
pub mod identity;
pub mod inflation;
pub mod jurisdiction;
pub mod light;
pub mod pos;
pub mod vm;

static GOV_STORE: Lazy<GovStore> = Lazy::new(|| GovStore::open("governance_db"));
static GOV_PARAMS: Lazy<Mutex<Params>> = Lazy::new(|| Mutex::new(Params::default()));
static LOCALNET_RECEIPTS: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_LOCALNET_DB_PATH").unwrap_or_else(|_| "localnet_db".into());
    Mutex::new(SimpleDb::open(&path))
});

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
    "dns.publish_record",
    "gateway.policy",
    "gateway.reads_since",
    #[cfg(feature = "telemetry")]
    "analytics",
    "microshard.roots.last",
    "mempool.stats",
    "net.peer_stats",
    "net.peer_stats_all",
    "net.peer_stats_reset",
    "net.peer_stats_export",
    "net.peer_stats_export_all",
    "net.peer_stats_persist",
    "net.peer_throttle",
    "net.backpressure_clear",
    "net.reputation_sync",
    "net.reputation_show",
    "net.dns_verify",
    "net.key_rotate",
    "net.handshake_failures",
    "kyc.verify",
    "pow.get_template",
    "dex_escrow_status",
    "dex_escrow_release",
    "dex_escrow_proof",
    "pow.submit",
    "inflation.params",
    "compute_market.stats",
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
    "light.latest_header",
    "service_badge_verify",
    "jurisdiction.status",
];

const ADMIN_METHODS: &[&str] = &[
    "set_snapshot_interval",
    "compute_arm_real",
    "compute_cancel_arm",
    "compute_back_to_dry_run",
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
];

const DEBUG_METHODS: &[&str] = &["set_difficulty", "start_mining", "stop_mining"];

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    _jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    badge: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RpcResponse {
    Result {
        jsonrpc: &'static str,
        result: serde_json::Value,
        id: Option<serde_json::Value>,
    },
    Error {
        jsonrpc: &'static str,
        error: RpcError,
        id: Option<serde_json::Value>,
    },
}

#[derive(Serialize, Debug)]
pub struct RpcError {
    code: i32,
    message: &'static str,
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
    params: &serde_json::Value,
    nonces: &Arc<Mutex<HashSet<u64>>>,
) -> Result<(), RpcError> {
    let nonce = params
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or(RpcError {
            code: -32602,
            message: "missing nonce",
        })?;
    let mut guard = nonces.lock().map_err(|_| RpcError {
        code: -32603,
        message: "internal error",
    })?;
    if !guard.insert(nonce) {
        return Err(RpcError {
            code: -32000,
            message: "replayed nonce",
        });
    }
    Ok(())
}

pub async fn handle_conn(
    stream: TcpStream,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<u64>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    cfg: Arc<RpcRuntimeConfig>,
) {
    let mut reader = BufReader::new(stream);
    let peer_ip = reader.get_ref().peer_addr().ok().map(|a| a.ip());

    // Read request line with timeout to avoid hanging connections.
    let mut line = String::new();
    match timeout(cfg.request_timeout, reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {}
        _ => return,
    }

    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    // Parse headers. Accept both CRLF and LF-only terminators.
    let mut content_len = 0usize;
    let mut expect_continue = false;
    let mut host = String::new();
    let mut origin = String::new();
    let mut auth: Option<String> = None;
    loop {
        line.clear();
        let read = match timeout(cfg.request_timeout, reader.read_line(&mut line)).await {
            Ok(Ok(n)) => n,
            _ => return,
        };
        if read == 0 {
            // EOF before headers complete
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let lower = line.to_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_len = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = lower.strip_prefix("expect:") {
            if val.trim().starts_with("100-continue") {
                expect_continue = true;
            }
        } else if let Some(val) = lower.strip_prefix("host:") {
            host = val.trim().to_string();
        } else if let Some(val) = lower.strip_prefix("origin:") {
            origin = val.trim().to_string();
        } else if lower.starts_with("authorization:") {
            auth = Some(line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string());
        }
    }

    // If the client sent 'Expect: 100-continue', acknowledge it to unblock senders.
    if expect_continue {
        let stream = reader.get_mut();
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
        let _ = stream.flush().await;
    }

    if method == "OPTIONS" {
        let mut stream = reader.into_inner();
        if cfg.cors_allow_origins.iter().any(|o| o == &origin) {
            let resp = format!("HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nContent-Length: 0\r\n\r\n");
            let _ = stream.write_all(resp.as_bytes()).await;
        } else {
            let _ = stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .await;
        }
        let _ = stream.shutdown().await;
        return;
    }

    if !cfg
        .allowed_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(host.trim()))
    {
        let mut stream = reader.into_inner();
        let _ = stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await;
        let _ = stream.shutdown().await;
        return;
    }

    if content_len > cfg.max_body_bytes {
        let mut stream = reader.into_inner();
        let _ = stream
            .write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n")
            .await;
        let _ = stream.shutdown().await;
        return;
    }

    if method == "GET" && path == "/badge/status" {
        let (active, last_mint, last_burn) = {
            let mut chain = bc.lock().unwrap_or_else(|e| e.into_inner());
            chain.check_badges();
            chain.badge_status()
        };
        let body = format!(
            "{{\"active\":{},\"last_mint\":{},\"last_burn\":{}}}",
            active,
            last_mint.map_or("null".into(), |v| v.to_string()),
            last_burn.map_or("null".into(), |v| v.to_string())
        );
        let mut headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        );
        if cfg.cors_allow_origins.iter().any(|o| o == &origin) {
            headers.push_str(&format!("Access-Control-Allow-Origin: {}\r\n", origin));
        }
        headers.push_str("\r\n");
        let resp = format!("{}{}", headers, body);
        let mut stream = reader.into_inner();
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        return;
    }
    if method == "GET" && path == "/dashboard" {
        let body = include_str!("../../../dashboard/index.html");
        let mut headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n",
            body.len()
        );
        if cfg.cors_allow_origins.iter().any(|o| o == &origin) {
            headers.push_str(&format!("Access-Control-Allow-Origin: {}\r\n", origin));
        }
        headers.push_str("\r\n");
        let resp = format!("{}{}", headers, body);
        let mut stream = reader.into_inner();
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        return;
    }

    // Read body (if any) with timeout; default to empty on missing Content-Length.
    let mut body_bytes = vec![0u8; content_len];
    if content_len > 0 {
        if timeout(cfg.request_timeout, reader.read_exact(&mut body_bytes))
            .await
            .ok()
            .is_none()
        {
            return;
        }
    }
    let body = String::from_utf8_lossy(&body_bytes);

    let req: Result<RpcRequest, _> = serde_json::from_str(&body);
    let resp = match req {
        Ok(r) => {
            let id = r.id.clone();
            let method_str = r.method.as_str();
            let authorized = match cfg.admin_token.as_ref() {
                Some(t) => {
                    let expected = format!("Bearer {t}");
                    match auth.as_deref() {
                        Some(h) => {
                            let a = h.as_bytes();
                            let b = expected.as_bytes();
                            a.len() == b.len() && a.ct_eq(b).into()
                        }
                        None => false,
                    }
                }
                None => false,
            };
            if DEBUG_METHODS.contains(&method_str) {
                if !cfg.enable_debug || !authorized {
                    RpcResponse::Error {
                        jsonrpc: "2.0",
                        error: RpcError {
                            code: -32601,
                            message: "method not found",
                        },
                        id,
                    }
                } else {
                    match dispatch(
                        &r,
                        Arc::clone(&bc),
                        Arc::clone(&mining),
                        Arc::clone(&nonces),
                        Arc::clone(&handles),
                    ) {
                        Ok(v) => RpcResponse::Result {
                            jsonrpc: "2.0",
                            result: v,
                            id,
                        },
                        Err(e) => RpcResponse::Error {
                            jsonrpc: "2.0",
                            error: e,
                            id,
                        },
                    }
                }
            } else if ADMIN_METHODS.contains(&method_str) {
                if !authorized {
                    RpcResponse::Error {
                        jsonrpc: "2.0",
                        error: RpcError {
                            code: -32601,
                            message: "method not found",
                        },
                        id,
                    }
                } else {
                    match dispatch(
                        &r,
                        Arc::clone(&bc),
                        Arc::clone(&mining),
                        Arc::clone(&nonces),
                        Arc::clone(&handles),
                    ) {
                        Ok(v) => RpcResponse::Result {
                            jsonrpc: "2.0",
                            result: v,
                            id,
                        },
                        Err(e) => RpcResponse::Error {
                            jsonrpc: "2.0",
                            error: e,
                            id,
                        },
                    }
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
                        RpcResponse::Error {
                            jsonrpc: "2.0",
                            error: RpcError {
                                code: -32601,
                                message: "method not found",
                            },
                            id,
                        }
                    } else {
                        if method_str == "net.peer_stats_export" {
                            if let Some(_path) = r.params.get("path").and_then(|v| v.as_str()) {
                                #[cfg(feature = "telemetry")]
                                log::info!(
                                    "peer_stats_export operator={:?} path={}",
                                    peer_ip,
                                    _path
                                );
                            } else {
                                #[cfg(feature = "telemetry")]
                                log::info!("peer_stats_export operator={:?}", peer_ip);
                            }
                        } else if method_str == "net.peer_stats_export_all" {
                            #[cfg(feature = "telemetry")]
                            log::info!("peer_stats_export_all operator={:?}", peer_ip);
                        } else if method_str == "net.peer_throttle" {
                            let _clear = r
                                .params
                                .get("clear")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            #[cfg(feature = "telemetry")]
                            log::info!("peer_throttle operator={:?} clear={}", peer_ip, _clear);
                        } else if method_str == "net.backpressure_clear" {
                            #[cfg(feature = "telemetry")]
                            log::info!("backpressure_clear operator={:?}", peer_ip);
                        }

                        match dispatch(
                            &r,
                            Arc::clone(&bc),
                            Arc::clone(&mining),
                            Arc::clone(&nonces),
                            Arc::clone(&handles),
                        ) {
                            Ok(v) => RpcResponse::Result {
                                jsonrpc: "2.0",
                                result: v,
                                id,
                            },
                            Err(e) => RpcResponse::Error {
                                jsonrpc: "2.0",
                                error: e,
                                id,
                            },
                        }
                    }
                } else {
                    match dispatch(
                        &r,
                        Arc::clone(&bc),
                        Arc::clone(&mining),
                        Arc::clone(&nonces),
                        Arc::clone(&handles),
                    ) {
                        Ok(v) => RpcResponse::Result {
                            jsonrpc: "2.0",
                            result: v,
                            id,
                        },
                        Err(e) => RpcResponse::Error {
                            jsonrpc: "2.0",
                            error: e,
                            id,
                        },
                    }
                }
            } else {
                RpcResponse::Error {
                    jsonrpc: "2.0",
                    error: RpcError {
                        code: -32601,
                        message: "method not found",
                    },
                    id,
                }
            }
        }
        Err(_) => RpcResponse::Error {
            jsonrpc: "2.0",
            error: RpcError {
                code: -32700,
                message: "parse error",
            },
            id: None,
        },
    };

    let body = serde_json::to_string(&resp).unwrap_or_else(|e| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32603, "message": e.to_string() },
            "id": serde_json::Value::Null
        })
        .to_string()
    });
    let mut stream = reader.into_inner();
    let mut headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if cfg.cors_allow_origins.iter().any(|o| o == &origin) {
        headers.push_str(&format!("Access-Control-Allow-Origin: {}\r\n", origin));
    }
    headers.push_str("\r\n");
    let response = format!("{}{}", headers, body);
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn dispatch(
    req: &RpcRequest,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<u64>>>,
    handles: Arc<Mutex<HandleRegistry>>,
) -> Result<serde_json::Value, RpcError> {
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
                    serde_json::json!({"status": "ok"})
                }
                Err(_) => serde_json::json!({"error": "lock poisoned"}),
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
                serde_json::json!({
                    "consumer": acct.balance.consumer,
                    "industrial": acct.balance.industrial,
                })
            } else {
                serde_json::json!({"consumer": 0, "industrial": 0})
            }
        }
        "settlement_status" => {
            let provider = req.params.get("provider").and_then(|v| v.as_str());
            let mode = match Settlement::mode() {
                SettleMode::DryRun => "dryrun",
                SettleMode::Real => "real",
                SettleMode::Armed { .. } => "armed",
            };
            if let Some(p) = provider {
                let bal = Settlement::balance(p);
                serde_json::json!({"mode": mode, "balance": bal})
            } else {
                serde_json::json!({"mode": mode})
            }
        }
        "settlement.audit" => {
            let res = Settlement::audit();
            serde_json::to_value(res).unwrap_or_else(|_| serde_json::json!([]))
        }
        "bridge.relayer_status" => {
            let id = req
                .params
                .get("relayer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            bridge::relayer_status(id)
        }
        "bridge.verify_deposit" => {
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
                    serde_json::from_value(v.clone()).map_err(|_| RpcError {
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
                    serde_json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            let rproof = req
                .params
                .get("relayer_proof")
                .ok_or(RpcError {
                    code: -32602,
                    message: "invalid params",
                })
                .and_then(|v| {
                    serde_json::from_value(v.clone()).map_err(|_| RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                })?;
            bridge::verify_deposit(relayer, user, amount, header, proof, rproof)?
        }
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
            let receipt: AssistReceipt = match bincode::deserialize(&bytes) {
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
                serde_json::json!({"status":"ignored"})
            } else {
                db.insert(&key, Vec::new());
                serde_json::json!({"status":"ok"})
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
        #[cfg(feature = "telemetry")]
        "analytics" => {
            let q: analytics::AnalyticsQuery = serde_json::from_value(req.params.clone())
                .unwrap_or(analytics::AnalyticsQuery {
                    domain: String::new(),
                });
            let stats = analytics::analytics(&crate::telemetry::READ_STATS, q);
            serde_json::to_value(stats).unwrap()
        }
        "microshard.roots.last" => {
            let n = req.params.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let roots = Settlement::recent_roots(n);
            serde_json::json!({"roots": roots})
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
            let (size, age_p50, age_p95, fee_p50, fee_p90) = guard.mempool_stats(lane);
            serde_json::json!({
                "size": size,
                "age_p50": age_p50,
                "age_p95": age_p95,
                "fee_p50": fee_p50,
                "fee_p90": fee_p90,
            })
        }
        "net.peer_stats" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = match hex::decode(id) {
                Ok(b) => b,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let pk: [u8; 32] = match bytes.try_into() {
                Ok(a) => a,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let m = net::peer_stats(&pk).ok_or(RpcError {
                code: -32602,
                message: "unknown peer",
            })?;
            serde_json::json!({
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
            serde_json::to_value(stats).unwrap()
        }
        "net.peer_stats_reset" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = match hex::decode(id) {
                Ok(b) => b,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let pk: [u8; 32] = match bytes.try_into() {
                Ok(a) => a,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            if net::reset_peer_metrics(&pk) {
                serde_json::json!({"status": "ok"})
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
                    Ok(over) => serde_json::json!({"status": "ok", "overwritten": over}),
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
                let bytes = match hex::decode(id) {
                    Ok(b) => b,
                    Err(_) => {
                        return Err(RpcError {
                            code: -32602,
                            message: "invalid params",
                        })
                    }
                };
                let pk: [u8; 32] = match bytes.try_into() {
                    Ok(a) => a,
                    Err(_) => {
                        return Err(RpcError {
                            code: -32602,
                            message: "invalid params",
                        })
                    }
                };
                match net::export_peer_stats(&pk, path) {
                    Ok(over) => serde_json::json!({"status": "ok", "overwritten": over}),
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
            serde_json::to_value(map).unwrap()
        }
        "net.peer_stats_persist" => match net::persist_peer_metrics() {
            Ok(()) => serde_json::json!({"status": "ok"}),
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
            let bytes = match hex::decode(id) {
                Ok(b) => b,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let pk: [u8; 32] = match bytes.try_into() {
                Ok(a) => a,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            if clear {
                if net::clear_throttle(&pk) {
                    serde_json::json!({"status": "ok"})
                } else {
                    return Err(RpcError {
                        code: -32602,
                        message: "unknown peer",
                    });
                }
            } else {
                net::throttle_peer(&pk, "manual");
                serde_json::json!({"status": "ok"})
            }
        }
        "net.backpressure_clear" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = match hex::decode(id) {
                Ok(b) => b,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            let pk: [u8; 32] = match bytes.try_into() {
                Ok(a) => a,
                Err(_) => {
                    return Err(RpcError {
                        code: -32602,
                        message: "invalid params",
                    })
                }
            };
            if net::clear_throttle(&pk) {
                serde_json::json!({"status": "ok"})
            } else {
                return Err(RpcError {
                    code: -32602,
                    message: "unknown peer",
                });
            }
        }
        "net.reputation_sync" => {
            net::reputation_sync();
            serde_json::json!({"status": "ok"})
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
                serde_json::json!({"status":"ok"})
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
            serde_json::json!({"failures": entries})
        }
        "net.config_reload" => {
            if crate::config::reload() {
                serde_json::json!({"status": "ok"})
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
                Ok(true) => serde_json::json!({"status": "verified"}),
                Ok(false) => serde_json::json!({"status": "denied"}),
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
            let tmpl = pow::template([0u8; 32], [0u8; 32], [0u8; 32], 1_000_000);
            serde_json::json!({
                "prev_hash": hex::encode(tmpl.prev_hash),
                "merkle_root": hex::encode(tmpl.merkle_root),
                "checkpoint_hash": hex::encode(tmpl.checkpoint_hash),
                "difficulty": tmpl.difficulty,
                "timestamp_millis": tmpl.timestamp_millis
            })
        }
        "pow.submit" => {
            let header_obj = req.params.get("header").ok_or(RpcError {
                code: -32602,
                message: "missing header",
            })?;
            let parse32 = |v: &serde_json::Value| -> Result<[u8; 32], RpcError> {
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
            let hdr = BlockHeader {
                prev_hash,
                merkle_root,
                checkpoint_hash,
                nonce,
                difficulty,
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
                serde_json::json!({"status":"accepted"})
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
            serde_json::to_value(light::latest_header(&guard)).unwrap()
        }
        "rent.escrow.balance" => {
            let esc = RentEscrow::open("rent_escrow.db");
            if let Some(id) = req.params.get("id").and_then(|v| v.as_str()) {
                serde_json::json!({"balance": esc.balance(id)})
            } else if let Some(acct) = req.params.get("account").and_then(|v| v.as_str()) {
                serde_json::json!({"balance": esc.balance_account(acct)})
            } else {
                serde_json::json!({"balance": 0})
            }
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
        "compute_market.scheduler_metrics" => compute_market::scheduler_metrics(),
        "compute_market.scheduler_stats" => compute_market::scheduler_stats(),
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
            serde_json::json!({"reloaded": ok})
        }
        "register_handle" => {
            check_nonce(&req.params, &nonces)?;
            match handles.lock() {
                Ok(mut reg) => match identity::register_handle(&req.params, &mut reg) {
                    Ok(v) => v,
                    Err(e) => serde_json::json!({"error": e.code()}),
                },
                Err(_) => serde_json::json!({"error": "lock poisoned"}),
            }
        }
        "resolve_handle" => match handles.lock() {
            Ok(reg) => identity::resolve_handle(&req.params, &reg),
            Err(_) => serde_json::json!({"address": null}),
        },
        "whoami" => match handles.lock() {
            Ok(reg) => identity::whoami(&req.params, &reg),
            Err(_) => serde_json::json!({"address": null, "handle": null}),
        },
        "record_le_request" => {
            check_nonce(&req.params, &nonces)?;
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
                    match crate::le_portal::record_request(&base, agency, case, jurisdiction) {
                        Ok(_) => serde_json::json!({"status": "ok"}),
                        Err(_) => serde_json::json!({"error": "io"}),
                    }
                }
                Err(_) => serde_json::json!({"error": "lock poisoned"}),
            }
        }
        "warrant_canary" => {
            check_nonce(&req.params, &nonces)?;
            let msg = req
                .params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    match crate::le_portal::record_canary(&base, msg) {
                        Ok(hash) => serde_json::json!({"hash": hash}),
                        Err(_) => serde_json::json!({"error": "io"}),
                    }
                }
                Err(_) => serde_json::json!({"error": "lock poisoned"}),
            }
        }
        "le.list_requests" => match bc.lock() {
            Ok(guard) => {
                let base = guard.path.clone();
                match crate::le_portal::list_requests(&base) {
                    Ok(v) => serde_json::to_value(v).unwrap_or_default(),
                    Err(_) => serde_json::json!({"error": "io"}),
                }
            }
            Err(_) => serde_json::json!({"error": "lock poisoned"}),
        },
        "le.record_action" => {
            check_nonce(&req.params, &nonces)?;
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
                    match crate::le_portal::record_action(&base, agency, action, jurisdiction) {
                        Ok(hash) => serde_json::json!({"hash": hash}),
                        Err(_) => serde_json::json!({"error": "io"}),
                    }
                }
                Err(_) => serde_json::json!({"error": "lock poisoned"}),
            }
        }
        "service_badge_verify" => {
            let badge = req
                .params
                .get("badge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            serde_json::json!({"valid": crate::service_badge::verify(badge)})
        }
        "submit_tx" => {
            check_nonce(&req.params, &nonces)?;
            let tx_hex = req.params.get("tx").and_then(|v| v.as_str()).unwrap_or("");
            match hex::decode(tx_hex)
                .ok()
                .and_then(|b| bincode::deserialize::<SignedTransaction>(&b).ok())
            {
                Some(tx) => match bc.lock() {
                    Ok(mut guard) => match guard.submit_transaction(tx) {
                        Ok(()) => serde_json::json!({"status": "ok"}),
                        Err(e) => serde_json::json!({"error": format!("{e:?}")}),
                    },
                    Err(_) => serde_json::json!({"error": "lock poisoned"}),
                },
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
            log::info!("snapshot_interval_changed {interval}");
            serde_json::json!({"status": "ok"})
        }
        "start_mining" => {
            if cfg.relay_only {
                serde_json::json!({
                    "error": {"code": -32075, "message": "relay_only"}
                })
            } else {
                check_nonce(&req.params, &nonces)?;
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
                serde_json::json!({"status": "ok"})
            }
        }
        "stop_mining" => {
            check_nonce(&req.params, &nonces)?;
            mining.store(false, Ordering::SeqCst);
            serde_json::json!({"status": "ok"})
        }
        "jurisdiction.status" => jurisdiction::status(&bc)?,
        "jurisdiction.set" => {
            check_nonce(&req.params, &nonces)?;
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
        "metrics" => {
            #[cfg(feature = "telemetry")]
            {
                let m = crate::gather_metrics().unwrap_or_default();
                serde_json::json!(m)
            }
            #[cfg(not(feature = "telemetry"))]
            {
                serde_json::json!("telemetry disabled")
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
                    serde_json::json!({"p25": p25, "median": median, "p75": p75})
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
            serde_json::json!({"status": "ok"})
        }
        "compute_cancel_arm" => {
            crate::compute_market::settlement::Settlement::cancel_arm();
            serde_json::json!({"status": "ok"})
        }
        "compute_back_to_dry_run" => {
            let reason = req
                .params
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            crate::compute_market::settlement::Settlement::back_to_dry_run(reason);
            serde_json::json!({"status": "ok"})
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
                serde_json::to_value(proof).map_err(|_| RpcError {
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
            serde_json::json!({"gas_used": gas})
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
            serde_json::json!({"trace": trace})
        }
        "vm.storage_read" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data = vm::storage_read(id).unwrap_or_default();
            serde_json::json!({"data": hex::encode(data)})
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
            serde_json::json!({"status": "ok"})
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
    let listener = TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?.to_string();
    let _ = ready.send(local);
    let nonces = Arc::new(Mutex::new(HashSet::new()));
    let handles = Arc::new(Mutex::new(HandleRegistry::open("identity_db")));
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
    let global = Arc::new(Semaphore::new(1024));
    loop {
        let (mut stream, addr) = listener.accept().await?;
        let permit = match global.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let Err(code) = limiter::check_client(
            &addr.ip(),
            &clients,
            tokens_per_sec,
            ban_secs,
            client_timeout,
        ) {
            telemetry_rpc_error(code);
            let err = RpcError {
                code: code.rpc_code(),
                message: code.message(),
            };
            let body = serde_json::to_string(&RpcResponse::Error {
                jsonrpc: "2.0",
                error: err,
                id: None,
            })
            .unwrap_or_else(|e| panic!("serialize RPC error response: {e}"));
            let response = format!(
                "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            continue;
        }
        let bc = Arc::clone(&bc);
        let mining = Arc::clone(&mining);
        let nonces = Arc::clone(&nonces);
        let handles_cl = Arc::clone(&handles);
        let cfg_cl = Arc::clone(&runtime_cfg);
        tokio::spawn(async move {
            let _p = permit;
            handle_conn(stream, bc, mining, nonces, handles_cl, cfg_cl).await;
        });
    }
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
    })
}
