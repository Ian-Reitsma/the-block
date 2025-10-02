//! Minimal HTTP gateway serving on-chain blobs and deterministic WASM.
//!
//! This server exposes zero-fee static file hosting backed by blob storage
//! along with optional dynamic endpoints powered by WASM. Every response
//! records a `ReadAck` that gateways later batch and anchor on-chain to claim
//! CT subsidies.

#![deny(warnings)]

use crypto_suite::hashing::blake3;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
mod rate_limit;
use once_cell::sync::Lazy;
use rate_limit::RateLimitFilter;
use signal_hook::consts::signal::SIGHUP;
use signal_hook::iterator::Signals;
use std::fs;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Body, Method, Request, Response, StatusCode};
use runtime::net::{TcpListener, TcpStream};
use runtime::sync::mpsc;
use runtime::ws::{self, Message as WsMessage, ServerStream};
use tokio::net::TcpStream as TokioTcpStream;
use wasmtime::{Engine, Func, Linker, Module, Store};

use crate::{
    exec,
    storage::pipeline,
    tx::web::{FuncTx, SiteManifestTx},
    ReadAck, StakeTable,
};
use tracing::warn;

/// Simple token bucket for per-IP throttling.
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn take(&mut self, rate: f64, burst: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Runs the gateway server on the given address.
pub async fn run(
    addr: SocketAddr,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let buckets: Arc<Mutex<HashMap<SocketAddr, Bucket>>> = Arc::new(Mutex::new(HashMap::new()));
    let filter = Arc::clone(&IP_FILTER);
    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let buckets = Arc::clone(&buckets);
        let stake = Arc::clone(&stake);
        let filter = Arc::clone(&filter);
        let read_tx = read_tx.clone();
        runtime::spawn(async move {
            let service = service_fn(move |req| {
                handle(
                    req,
                    remote_addr,
                    Arc::clone(&buckets),
                    Arc::clone(&filter),
                    Arc::clone(&stake),
                    read_tx.clone(),
                )
            });
            if let Err(err) = http1::Builder::new()
                .serve_connection(stream, service)
                .with_upgrades()
                .await
            {
                warn!(
                    target: "gateway",
                    addr = %remote_addr,
                    error = %err,
                    "connection closed with error"
                );
            }
        });
    }
    #[allow(unreachable_code)]
    Ok(())
}

async fn handle(
    req: Request<Body>,
    ip: SocketAddr,
    buckets: Arc<Mutex<HashMap<SocketAddr, Bucket>>>,
    filter: Arc<Mutex<RateLimitFilter>>,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
) -> Result<Response<Body>, hyper::Error> {
    if !check_bucket(&ip, &buckets, &filter) {
        crate::net::peer::record_ip_drop(&ip);
        return Ok(Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .body(Body::empty())
            .unwrap());
    }
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !stake.has_stake(&host) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("domain stake required"))
            .unwrap());
    }
    let path = req.uri().path().to_string();
    if path.starts_with("/ws/peer_metrics") {
        if !ip.ip().is_loopback() {
            return Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::empty())
                .unwrap());
        }
        return ws_peer_metrics(req).await;
    }
    if path.starts_with("/api/") {
        return handle_func(host, &path[4..], req, read_tx).await;
    }
    handle_static(host, &path, read_tx).await
}

fn check_bucket(
    ip: &SocketAddr,
    buckets: &Arc<Mutex<HashMap<SocketAddr, Bucket>>>,
    filter: &Arc<Mutex<RateLimitFilter>>,
) -> bool {
    let key = ip_key(ip);
    if filter.lock().unwrap().contains(key) {
        crate::net::peer::record_ip_drop(ip);
        return false;
    }
    let mut map = buckets.lock().unwrap();
    let b = map.entry(*ip).or_insert(Bucket {
        tokens: 1.0,
        last: Instant::now(),
    });
    if b.take(20.0, 20.0) {
        true
    } else {
        filter.lock().unwrap().insert(key);
        crate::net::peer::record_ip_drop(ip);
        false
    }
}

static IP_FILTER: Lazy<Arc<Mutex<RateLimitFilter>>> =
    Lazy::new(|| Arc::new(Mutex::new(RateLimitFilter::new())));
static BLOCKLIST_PATH: Lazy<Arc<Mutex<Option<String>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

pub fn load_blocklist(path: &str) {
    if let Ok(data) = fs::read_to_string(path) {
        let mut keys = Vec::new();
        for line in data.lines() {
            if let Ok(addr) = line.parse::<std::net::IpAddr>() {
                let key = match addr {
                    std::net::IpAddr::V4(v4) => u32::from(v4) as u64,
                    std::net::IpAddr::V6(v6) => {
                        let o = v6.octets();
                        let mut b = [0u8; 8];
                        b.copy_from_slice(&o[0..8]);
                        u64::from_le_bytes(b)
                    }
                };
                keys.push(key);
            }
        }
        let mut guard = IP_FILTER.lock().unwrap();
        guard.replace(keys);
    }
    *BLOCKLIST_PATH.lock().unwrap() = Some(path.to_string());
}

/// Install a SIGHUP handler that reloads the blocklist file when triggered.
pub fn install_blocklist_reload() {
    let path = BLOCKLIST_PATH.lock().unwrap().clone();
    if let Some(p) = path {
        std::thread::spawn(move || {
            let mut signals = Signals::new([SIGHUP]).expect("signals");
            for _ in signals.forever() {
                load_blocklist(&p);
            }
        });
    }
}

pub fn ip_key(ip: &SocketAddr) -> u64 {
    match ip.ip() {
        IpAddr::V4(v4) => u32::from(v4) as u64,
        IpAddr::V6(v6) => {
            let o = v6.octets();
            let mut b = [0u8; 8];
            b.copy_from_slice(&o[0..8]);
            u64::from_le_bytes(b)
        }
    }
}

// SIMD-aware rate limit filter lives in rate_limit.rs

async fn ws_peer_metrics(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if req.method() != Method::GET {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::from("websocket upgrade requires GET"))
            .unwrap());
    }

    let headers = req.headers();
    let upgrade_ok = headers
        .get("Upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    let connection_ok = headers
        .get("Connection")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false);
    if !upgrade_ok || !connection_ok {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("websocket upgrade required"))
            .unwrap());
    }

    let key = match headers
        .get("Sec-WebSocket-Key")
        .and_then(|v| v.to_str().ok())
    {
        Some(key) => key.to_string(),
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("missing Sec-WebSocket-Key"))
                .unwrap())
        }
    };

    let version_ok = headers
        .get("Sec-WebSocket-Version")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "13")
        .unwrap_or(false);
    if !version_ok {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("unsupported websocket version"))
            .unwrap());
    }

    let accept = ws::handshake_accept(&key);
    let response = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Accept", accept)
        .body(Body::empty())
        .unwrap();

    runtime::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => match upgraded.downcast::<TokioTcpStream>() {
                Ok(stream) => {
                    if let Err(err) = handle_peer_metrics_ws(stream).await {
                        warn!(
                            target: "gateway",
                            error = %err,
                            "peer metrics websocket closed with error"
                        );
                    }
                }
                Err(_) => {
                    warn!(
                        target: "gateway",
                        "failed to downcast upgraded connection to TcpStream"
                    );
                }
            },
            Err(err) => {
                warn!(
                    target: "gateway",
                    error = %err,
                    "websocket upgrade failed"
                );
            }
        }
    });

    Ok(response)
}

async fn handle_peer_metrics_ws(stream: TokioTcpStream) -> std::io::Result<()> {
    let mut ws = ServerStream::new(TcpStream::from_tokio(stream));
    let mut rx = crate::net::peer::subscribe_peer_metrics();

    loop {
        runtime::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(snap) => {
                        let payload = serde_json::to_string(&snap).unwrap();
                        ws.send(WsMessage::Text(payload)).await?;
                    }
                    Err(_) => break,
                }
            }
            frame = ws.recv() => {
                match frame {
                    Ok(Some(WsMessage::Close(_))) | Ok(None) => break,
                    Ok(Some(WsMessage::Ping(_))) | Ok(Some(WsMessage::Pong(_))) => {}
                    Ok(Some(_)) => {}
                    Err(err) => return Err(err),
                }
            }
        }
    }

    Ok(())
}

async fn handle_static(
    domain: String,
    path: &str,
    read_tx: mpsc::Sender<ReadAck>,
) -> Result<Response<Body>, hyper::Error> {
    // look up manifest and blob bytes
    let blob = pipeline::fetch_blob(&domain, path).unwrap_or_default();
    let bytes = blob.len() as u64;
    #[cfg(feature = "telemetry")]
    crate::telemetry::READ_STATS.record(&domain, bytes);
    let ack = ReadAck {
        manifest: [0; 32],
        path_hash: blake3::hash(path.as_bytes()).into(),
        bytes,
        ts: now_ts(),
        client_hash: blake3::hash(domain.as_bytes()).into(),
        pk: [0u8; 32],
        sig: [0u8; 64],
    };
    let _ = read_tx.send(ack).await;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(blob))
        .unwrap())
}

async fn handle_func(
    domain: String,
    api: &str,
    req: Request<Body>,
    read_tx: mpsc::Sender<ReadAck>,
) -> Result<Response<Body>, hyper::Error> {
    let wasm = pipeline::fetch_wasm(&domain).unwrap_or_default();
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).map_err(|_| {
        hyper::Error::new_std(std::io::Error::new(std::io::ErrorKind::Other, "wasm"))
    })?;
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let func = linker
        .instantiate(&mut store, &module)
        .and_then(|i| i.get_func(&mut store, "handler"))
        .ok();
    if let Some(f) = func {
        let body_bytes = hyper::body::to_bytes(req.into_body())
            .await
            .unwrap_or_default();
        let start = Instant::now();
        let res = f.call(&mut store, &[], &mut []).map(|_| body_bytes);
        match res {
            Ok(out) => {
                let cpu_ms = start.elapsed().as_millis() as u64;
                let bytes_out = out.len() as u64;
                let func_id: [u8; 32] = blake3::hash(&wasm).into();
                let _ = exec::record(
                    &domain,
                    func_id,
                    bytes_out,
                    cpu_ms,
                    [0u8; 32],
                    Vec::new(),
                    Vec::new(),
                    &crate::logging::corr_id_hash(&func_id),
                );
                #[cfg(feature = "telemetry")]
                crate::telemetry::READ_STATS.record(&domain, bytes_out);
                let ack = ReadAck {
                    manifest: [0; 32],
                    path_hash: blake3::hash(api.as_bytes()).into(),
                    bytes: bytes_out,
                    ts: now_ts(),
                    client_hash: blake3::hash(domain.as_bytes()).into(),
                    pk: [0u8; 32],
                    sig: [0u8; 64],
                };
                let _ = read_tx.send(ack).await;
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(out))
                    .unwrap())
            }
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("exec failed"))
                .unwrap()),
        }
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("no func"))
            .unwrap())
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Trait for looking up domain stake deposits.
pub trait StakeTable {
    fn has_stake(&self, domain: &str) -> bool;
}
