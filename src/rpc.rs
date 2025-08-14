#[cfg(feature = "telemetry")]
use crate::gather_metrics;
use crate::{Blockchain, SignedTransaction};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

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

fn handle_conn(mut stream: TcpStream, bc: Arc<Mutex<Blockchain>>, mining: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }

    let mut content_len = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        if line == "\r\n" {
            break;
        }
        if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
            content_len = val.trim().parse().unwrap_or(0);
        }
    }

    let mut body_bytes = vec![0u8; content_len];
    if reader.read_exact(&mut body_bytes).is_err() {
        return;
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
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn dispatch(
    req: &RpcRequest,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
) -> Result<serde_json::Value, RpcError> {
    Ok(match req.method.as_str() {
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
                thread::spawn(move || {
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
                let m = gather_metrics().unwrap_or_default();
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

pub fn spawn_rpc_server(
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    addr: &str,
) -> std::io::Result<(String, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let bc = Arc::clone(&bc);
    let mining = Arc::clone(&mining);
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let bc = Arc::clone(&bc);
                let mining = Arc::clone(&mining);
                thread::spawn(move || handle_conn(stream, bc, mining));
            }
        }
    });
    Ok((local.to_string(), handle))
}
