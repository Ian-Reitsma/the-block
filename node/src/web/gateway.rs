//! Minimal HTTP gateway serving on-chain blobs and deterministic WASM.
//!
//! This server exposes zero-fee static file hosting backed by blob storage
//! along with optional dynamic endpoints powered by WASM. Every response
//! records a `ReadAck` that gateways later batch and anchor on-chain to claim
//! CT subsidies.

use std::{collections::HashMap, net::SocketAddr, sync::{Arc, Mutex}, time::{Duration, Instant}};

use hyper::{Body, Request, Response, Server, Method, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use tokio::sync::mpsc;
use wasmtime::{Engine, Store, Module, Linker, Func};

use crate::{storage::pipeline, tx::web::{SiteManifestTx, FuncTx}, ReadAck, StakeTable, exec};

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
pub async fn run(addr: SocketAddr, stake: Arc<dyn StakeTable + Send + Sync>, read_tx: mpsc::Sender<ReadAck>) -> anyhow::Result<()> {
    let buckets: Arc<Mutex<HashMap<SocketAddr, Bucket>>> = Arc::new(Mutex::new(HashMap::new()));
    let make = make_service_fn(move |conn: &hyper::server::conn::AddrStream| {
        let ip = conn.remote_addr();
        let buckets = Arc::clone(&buckets);
        let stake = Arc::clone(&stake);
        let read_tx = read_tx.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                handle(req, ip, Arc::clone(&buckets), Arc::clone(&stake), read_tx.clone())
            }))
        }
    });
    Server::bind(&addr).serve(make).await?;
    Ok(())
}

async fn handle(
    req: Request<Body>,
    ip: SocketAddr,
    buckets: Arc<Mutex<HashMap<SocketAddr, Bucket>>>,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
) -> Result<Response<Body>, hyper::Error> {
    if !check_bucket(&ip, &buckets) {
        return Ok(Response::builder().status(StatusCode::TOO_MANY_REQUESTS).body(Body::empty()).unwrap());
    }
    let host = req.headers().get("host").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    if !stake.has_stake(&host) {
        return Ok(Response::builder().status(StatusCode::FORBIDDEN).body(Body::from("domain stake required")) .unwrap());
    }
    let path = req.uri().path().to_string();
    if path.starts_with("/api/") {
        return handle_func(host, &path[4..], req, read_tx).await;
    }
    handle_static(host, &path, read_tx).await
}

fn check_bucket(ip: &SocketAddr, buckets: &Arc<Mutex<HashMap<SocketAddr, Bucket>>>) -> bool {
    let mut map = buckets.lock().unwrap();
    let b = map.entry(*ip).or_insert(Bucket{tokens: 1.0, last: Instant::now()});
    b.take(20.0, 20.0)
}

async fn handle_static(domain: String, path: &str, read_tx: mpsc::Sender<ReadAck>) -> Result<Response<Body>, hyper::Error> {
    // look up manifest and blob bytes
    let blob = pipeline::fetch_blob(&domain, path).unwrap_or_default();
    let bytes = blob.len() as u64;
    let ack = ReadAck {
        manifest: [0;32],
        path_hash: blake3::hash(path.as_bytes()).into(),
        bytes,
        ts: now_ts(),
        client_hash: blake3::hash(domain.as_bytes()).into(),
        pk: [0u8;32],
        sig: [0u8;64],
    };
    let _ = read_tx.send(ack).await;
    Ok(Response::builder().status(StatusCode::OK).body(Body::from(blob)).unwrap())
}

async fn handle_func(domain: String, api: &str, req: Request<Body>, read_tx: mpsc::Sender<ReadAck>) -> Result<Response<Body>, hyper::Error> {
    let wasm = pipeline::fetch_wasm(&domain).unwrap_or_default();
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).map_err(|_| hyper::Error::new_std(std::io::Error::new(std::io::ErrorKind::Other, "wasm")))?;
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let func = linker.instantiate(&mut store, &module).and_then(|i| i.get_func(&mut store, "handler")).ok();
    if let Some(f) = func {
        let body_bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
        let start = Instant::now();
        let res = f.call(&mut store, &[], &mut []).map(|_| body_bytes);
        match res {
            Ok(out) => {
                let cpu_ms = start.elapsed().as_millis() as u64;
                let bytes_out = out.len() as u64;
                let func_id: [u8;32] = blake3::hash(&wasm).into();
                let _ = exec::record(&domain, func_id, bytes_out, cpu_ms, [0u8;32], Vec::new(), Vec::new());
                let ack = ReadAck {
                    manifest: [0;32],
                    path_hash: blake3::hash(api.as_bytes()).into(),
                    bytes: bytes_out,
                    ts: now_ts(),
                    client_hash: blake3::hash(domain.as_bytes()).into(),
                    pk: [0u8;32],
                    sig: [0u8;64],
                };
                let _ = read_tx.send(ack).await;
                Ok(Response::builder().status(StatusCode::OK).body(Body::from(out)).unwrap())
            }
            Err(_) => Ok(Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::from("exec failed")).unwrap()),
        }
    } else {
        Ok(Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("no func" )).unwrap())
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// Trait for looking up domain stake deposits.
pub trait StakeTable {
    fn has_stake(&self, domain: &str) -> bool;
}
