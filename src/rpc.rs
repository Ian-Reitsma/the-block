use crate::{Blockchain, SignedTransaction};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

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
struct RpcError {
    code: i32,
    message: &'static str,
}

async fn handle_conn(stream: TcpStream, bc: Arc<Mutex<Blockchain>>, mining: Arc<AtomicBool>) {
    let mut reader = BufReader::new(stream);

    // Read request line with timeout to avoid hanging connections.
    let mut line = String::new();
    match timeout(Duration::from_secs(3), reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {}
        _ => return,
    }

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
            match dispatch(&r, bc, mining) {
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
        "submit_tx" => {
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
        "start_mining" => {
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
    loop {
        let (stream, _) = listener.accept().await?;
        let bc = Arc::clone(&bc);
        let mining = Arc::clone(&mining);
        tokio::spawn(async move {
            handle_conn(stream, bc, mining).await;
        });
    }
}
