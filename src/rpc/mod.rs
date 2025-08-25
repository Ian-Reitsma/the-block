use crate::{
    governance::{GovStore, Params},
    identity::handle_registry::HandleRegistry,
    Blockchain, SignedTransaction,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::time::{timeout, Duration};

pub mod limiter;
use limiter::{ClientState, RpcClientErrorCode};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub mod governance;
pub mod identity;

static GOV_STORE: Lazy<GovStore> = Lazy::new(|| GovStore::open("governance_db"));
static GOV_PARAMS: Lazy<Mutex<Params>> = Lazy::new(|| Mutex::new(Params::default()));

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    _jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    #[serde(default)]
    id: Option<serde_json::Value>,
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

#[derive(Serialize)]
pub struct RpcError {
    code: i32,
    message: &'static str,
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

async fn handle_conn(
    stream: TcpStream,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<u64>>>,
    handles: Arc<Mutex<HandleRegistry>>,
) {
    let mut reader = BufReader::new(stream);

    // Read request line with timeout to avoid hanging connections.
    let mut line = String::new();
    match timeout(Duration::from_secs(3), reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {}
        _ => return,
    }

    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    // Parse headers. Accept both CRLF and LF-only terminators.
    let mut content_len = 0usize;
    let mut expect_continue = false;
    loop {
        line.clear();
        let read = match timeout(Duration::from_secs(3), reader.read_line(&mut line)).await {
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
        }
    }

    // If the client sent 'Expect: 100-continue', acknowledge it to unblock senders.
    if expect_continue {
        let stream = reader.get_mut();
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
        let _ = stream.flush().await;
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
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut stream = reader.into_inner();
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        return;
    }

    // Read body (if any) with timeout; default to empty on missing Content-Length.
    let mut body_bytes = vec![0u8; content_len];
    if content_len > 0 {
        if timeout(Duration::from_secs(3), reader.read_exact(&mut body_bytes))
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
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
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
                    match crate::le_portal::record_request(&base, agency, case) {
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
        "stop_mining" => {
            check_nonce(&req.params, &nonces)?;
            mining.store(false, Ordering::SeqCst);
            serde_json::json!({"status": "ok"})
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
        "price_board_get" => match crate::compute_market::price_board::bands() {
            Some((p25, median, p75)) => {
                serde_json::json!({"p25": p25, "median": median, "p75": p75})
            }
            None => {
                return Err(crate::compute_market::MarketError::NoPriceData.into());
            }
        },
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
        "gov_list" => governance::gov_list(&GOV_STORE)?,
        "gov_params" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let params = GOV_PARAMS.lock().unwrap();
            governance::gov_params(&params, epoch)?
        }
        "gov_rollback_last" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let mut params = GOV_PARAMS.lock().unwrap();
            governance::gov_rollback_last(&GOV_STORE, &mut params, epoch)?
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
    loop {
        let (mut stream, addr) = listener.accept().await?;
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
        tokio::spawn(async move {
            handle_conn(stream, bc, mining, nonces, handles_cl).await;
        });
    }
}
