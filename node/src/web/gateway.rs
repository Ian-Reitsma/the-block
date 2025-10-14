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
    time::Instant,
};
mod rate_limit;
use concurrency::Lazy;
use rate_limit::RateLimitFilter;
use std::fs;
use sys::signals::{Signals, SIGHUP};

use crate::{storage::pipeline, vm::wasm, ReadAck, StakeTable};
use httpd::{
    serve, HttpError, Method, Request, Response, Router, ServerConfig, StatusCode,
    WebSocketRequest, WebSocketResponse,
};
use runtime::net::TcpListener;
use runtime::sync::mpsc;
use runtime::ws::Message as WsMessage;

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

#[derive(Clone)]
struct GatewayState {
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
    buckets: Arc<Mutex<HashMap<SocketAddr, Bucket>>>,
    filter: Arc<Mutex<RateLimitFilter>>,
}

#[derive(Clone)]
struct DynamicFunc {
    wasm: Vec<u8>,
    gas_limit: u64,
}

static DYNAMIC_FUNCS: Lazy<Mutex<HashMap<(String, String), DynamicFunc>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_dynamic(domain: &str, path: &str, wasm: Vec<u8>, gas_limit: u64) {
    DYNAMIC_FUNCS.lock().unwrap().insert(
        (domain.to_string(), path.to_string()),
        DynamicFunc { wasm, gas_limit },
    );
}

fn lookup_dynamic(domain: &str, path: &str) -> Option<DynamicFunc> {
    DYNAMIC_FUNCS
        .lock()
        .unwrap()
        .get(&(domain.to_string(), path.to_string()))
        .cloned()
}

#[cfg(test)]
fn clear_dynamic_registry() {
    DYNAMIC_FUNCS.lock().unwrap().clear();
}

impl GatewayState {
    fn check_bucket(&self, ip: &SocketAddr) -> bool {
        let key = ip_key(ip);
        if self.filter.lock().unwrap().contains(key) {
            crate::net::peer::record_ip_drop(ip);
            return false;
        }
        let mut map = self.buckets.lock().unwrap();
        let bucket = map.entry(*ip).or_insert(Bucket {
            tokens: 1.0,
            last: Instant::now(),
        });
        if bucket.take(20.0, 20.0) {
            true
        } else {
            self.filter.lock().unwrap().insert(key);
            crate::net::peer::record_ip_drop(ip);
            false
        }
    }

    fn authorize(&self, req: &Request<GatewayState>) -> Result<String, Response> {
        let ip = req.remote_addr();
        if !self.check_bucket(&ip) {
            return Err(Response::new(StatusCode::TOO_MANY_REQUESTS).close());
        }
        let host = req.header("host").unwrap_or("").to_string();
        if !self.stake.has_stake(&host) {
            return Err(
                Response::new(StatusCode::FORBIDDEN).with_body(b"domain stake required".to_vec())
            );
        }
        Ok(host)
    }
}

/// Runs the gateway server on the given address.
pub async fn run(
    addr: SocketAddr,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
) -> diagnostics::anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let state = GatewayState {
        stake,
        read_tx,
        buckets: Arc::new(Mutex::new(HashMap::new())),
        filter: Arc::clone(&IP_FILTER),
    };
    let router = Router::new(state)
        .upgrade("/ws/peer_metrics", ws_peer_metrics)
        .route(Method::Get, "/api/*tail", handle_api)
        .route(Method::Post, "/api/*tail", handle_api)
        .route(Method::Get, "/*path", handle_static);
    serve(listener, router, ServerConfig::default()).await?;
    Ok(())
}

static IP_FILTER: Lazy<Arc<Mutex<RateLimitFilter>>> =
    Lazy::new(|| Arc::new(Mutex::new(RateLimitFilter::new())));
static BLOCKLIST_PATH: Lazy<Arc<Mutex<Option<String>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

pub fn load_blocklist(path: &str) {
    if let Ok(data) = fs::read_to_string(path) {
        let mut keys = Vec::new();
        for line in data.lines() {
            if let Ok(addr) = line.parse::<IpAddr>() {
                let key = match addr {
                    IpAddr::V4(v4) => u32::from(v4) as u64,
                    IpAddr::V6(v6) => {
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

async fn ws_peer_metrics(
    req: Request<GatewayState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = req.state().clone();
    if let Err(response) = state.authorize(&req) {
        return Ok(WebSocketResponse::reject(response));
    }
    if !req.remote_addr().ip().is_loopback() {
        return Ok(WebSocketResponse::reject(
            Response::new(StatusCode::FORBIDDEN).with_body(Vec::new()),
        ));
    }
    Ok(WebSocketResponse::accept(move |mut stream| {
        let mut rx = crate::net::peer::subscribe_peer_metrics();
        async move {
            loop {
                match runtime::select2(rx.recv(), stream.recv()).await {
                    runtime::Either::First(msg) => match msg {
                        Ok(snap) => {
                            let payload = json::to_string(&snap).unwrap();
                            stream.send(WsMessage::Text(payload)).await?;
                        }
                        Err(_) => break,
                    },
                    runtime::Either::Second(frame) => match frame {
                        Ok(Some(WsMessage::Close(_))) | Ok(None) => break,
                        Ok(Some(WsMessage::Ping(_))) | Ok(Some(WsMessage::Pong(_))) => {}
                        Ok(Some(_)) => {}
                        Err(err) => return Err(HttpError::from(err)),
                    },
                }
            }
            Ok(())
        }
    }))
}

async fn handle_static(req: Request<GatewayState>) -> Result<Response, HttpError> {
    let state = req.state().clone();
    let domain = match state.authorize(&req) {
        Ok(host) => host,
        Err(response) => return Ok(response),
    };
    let path = req.path();
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
    let _ = state.read_tx.send(ack).await;
    Ok(Response::new(StatusCode::OK).with_body(blob))
}

async fn handle_api(mut req: Request<GatewayState>) -> Result<Response, HttpError> {
    let state = req.state().clone();
    let domain = match state.authorize(&req) {
        Ok(host) => host,
        Err(response) => return Ok(response),
    };
    let tail = req.param("tail").unwrap_or("");
    handle_func(domain, tail, req, state).await
}

async fn handle_func(
    domain: String,
    api: &str,
    mut req: Request<GatewayState>,
    state: Arc<GatewayState>,
) -> Result<Response, HttpError> {
    if let Some(func) = lookup_dynamic(&domain, api) {
        let mut meter = wasm::GasMeter::new(func.gas_limit);
        match wasm::execute(&func.wasm, req.body_bytes(), &mut meter) {
            Ok(bytes) => Ok(Response::new(StatusCode::OK).with_body(bytes)),
            Err(err) => Ok(Response::new(StatusCode::BAD_REQUEST)
                .with_body(format!("wasm execution failed: {err}\n").into_bytes())
                .close()),
        }
    } else {
        let mut body = format!("dynamic endpoint '{api}' not registered\n").into_bytes();
        let _ = pipeline::fetch_wasm(&domain);
        Ok(Response::new(StatusCode::NOT_FOUND).with_body(body).close())
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpd::{Method, Router, StatusCode};
    use runtime::sync::mpsc;
    use std::collections::{HashMap, HashSet};

    struct StaticStake {
        allowed: HashSet<String>,
    }

    impl StakeTable for StaticStake {
        fn has_stake(&self, domain: &str) -> bool {
            self.allowed.contains(domain)
        }
    }

    fn state_with_domains(domains: &[&str]) -> GatewayState {
        let allowed = domains
            .iter()
            .map(|d| d.to_string())
            .collect::<HashSet<_>>();
        let (tx, _rx) = mpsc::channel(16);
        GatewayState {
            stake: Arc::new(StaticStake { allowed }),
            read_tx: tx,
            buckets: Arc::new(Mutex::new(HashMap::new())),
            filter: Arc::new(Mutex::new(RateLimitFilter::new())),
        }
    }

    #[test]
    fn authorize_allows_staked_domains() {
        let state = state_with_domains(&["allowed.test"]);
        let router = Router::new(state.clone());
        let request = router.request_builder().host("allowed.test").build();
        let host = state.authorize(&request).expect("authorized host");
        assert_eq!(host, "allowed.test");
    }

    #[test]
    fn authorize_rejects_missing_stake() {
        let state = state_with_domains(&[]);
        let router = Router::new(state.clone());
        let request = router.request_builder().host("unbonded.test").build();
        let response = state.authorize(&request).expect_err("missing stake");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.body(), b"domain stake required");
    }

    #[test]
    fn authorize_rate_limits_when_bucket_exhausted() {
        let state = state_with_domains(&["throttle.test"]);
        let router = Router::new(state.clone());
        let request = router
            .request_builder()
            .host("throttle.test")
            .remote_addr("127.0.0.1:9000".parse().unwrap())
            .build();
        assert!(state.authorize(&request).is_ok());
        let response = state.authorize(&request).expect_err("rate limited");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn dynamic_execution_returns_bytes() {
        clear_dynamic_registry();
        let module = {
            let mut buf = Vec::new();
            buf.extend_from_slice(&wasm::MAGIC);
            buf.push(wasm::VERSION_V1);
            buf.extend_from_slice(&[
                wasm::opcodes::PUSH_INPUT,
                0,
                wasm::opcodes::PUSH_INPUT,
                1,
                wasm::opcodes::ADD_I64,
                wasm::opcodes::RETURN,
                1,
            ]);
            buf
        };
        register_dynamic("dyn.test", "/sum", module, 64);

        let state = state_with_domains(&["dyn.test"]);
        let router = Router::new(state.clone()).route(Method::Post, "/api/*tail", handle_api);
        let mut body = Vec::new();
        body.extend_from_slice(&3i64.to_le_bytes());
        body.extend_from_slice(&7i64.to_le_bytes());
        let request = router
            .request_builder()
            .host("dyn.test")
            .method(Method::Post)
            .path("/api/sum")
            .body(body)
            .build();
        let response = router.handle(request).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), 10i64.to_le_bytes());
    }
}
