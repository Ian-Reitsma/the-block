#[cfg(feature = "telemetry")]
use crate::gather_metrics;
use crate::{Blockchain, SignedTransaction};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

#[derive(Deserialize)]
struct RpcRequest {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Serialize)]
struct RpcResponse {
    result: serde_json::Value,
}

fn handle_conn(mut stream: TcpStream, bc: Arc<Mutex<Blockchain>>, mining: Arc<AtomicBool>) {
    let mut buf = [0u8; 4096];
    if let Ok(n) = stream.read(&mut buf) {
        let req_str = String::from_utf8_lossy(&buf[..n]);
        let body = if let Some(idx) = req_str.find("\r\n\r\n") {
            &req_str[idx + 4..]
        } else {
            ""
        };
        let req: Result<RpcRequest, _> = serde_json::from_str(body);
        let resp = match req {
            Ok(r) => dispatch(r, bc, mining),
            Err(e) => serde_json::json!({"error": e.to_string()}),
        };
        let body = serde_json::to_string(&RpcResponse { result: resp })
            .unwrap_or_else(|e| format!(r#"{{"error":"{e}"}}"#));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    }
}

fn dispatch(
    req: RpcRequest,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
) -> serde_json::Value {
    match req.method.as_str() {
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
                None => serde_json::json!({"error": "invalid tx"}),
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
        _ => serde_json::json!({"error": "unknown method"}),
    }
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
                handle_conn(stream, Arc::clone(&bc), Arc::clone(&mining));
            }
        }
    });
    Ok((local.to_string(), handle))
}
