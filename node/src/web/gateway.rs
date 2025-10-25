//! Minimal HTTP gateway serving on-chain blobs and deterministic WASM.
//!
//! This server exposes zero-fee static file hosting backed by blob storage
//! along with optional dynamic endpoints powered by WASM. Every response
//! records a `ReadAck` that gateways later batch and anchor on-chain to claim
//! CT subsidies.

#![deny(warnings)]

use ad_market::{ImpressionContext, MarketplaceHandle, ReservationKey};
use concurrency::Lazy;
use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::hex;
use std::fs;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Instant,
};
use sys::signals::{Signals, SIGHUP};

use crate::web::rate_limit::RateLimitFilter;
use crate::{service_badge, storage::pipeline, vm::wasm, ReadAck};
use foundation_serialization::json;
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
    market: Option<MarketplaceHandle>,
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

fn normalize_dynamic_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('/') {
        rest.to_string()
    } else {
        path.to_string()
    }
}

pub fn register_dynamic(domain: &str, path: &str, wasm: Vec<u8>, gas_limit: u64) {
    DYNAMIC_FUNCS.lock().unwrap().insert(
        (domain.to_string(), normalize_dynamic_path(path)),
        DynamicFunc { wasm, gas_limit },
    );
}

fn lookup_dynamic(domain: &str, path: &str) -> Option<DynamicFunc> {
    DYNAMIC_FUNCS
        .lock()
        .unwrap()
        .get(&(domain.to_string(), normalize_dynamic_path(path)))
        .cloned()
}

const HEADER_ACK_MANIFEST: &str = "x-theblock-ack-manifest";
const HEADER_ACK_PUBKEY: &str = "x-theblock-ack-pk";
const HEADER_ACK_SIGNATURE: &str = "x-theblock-ack-sig";
const HEADER_ACK_BYTES: &str = "x-theblock-ack-bytes";
const HEADER_ACK_TIMESTAMP: &str = "x-theblock-ack-ts";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckParseError {
    Missing(&'static str),
    Decode(&'static str),
    Length {
        header: &'static str,
        expected: usize,
        actual: usize,
    },
    ParseInt(&'static str),
    BytesMismatch {
        declared: u64,
        actual: u64,
    },
    InvalidSignature,
}

impl AckParseError {
    #[cfg(feature = "legacy-read-acks")]
    fn is_missing(self) -> bool {
        matches!(self, AckParseError::Missing(_))
    }
}

fn ack_error_response(err: AckParseError) -> Response {
    let message = match err {
        AckParseError::Missing(header) => format!("missing {header} header"),
        AckParseError::Decode(header) => format!("failed to decode {header}"),
        AckParseError::Length {
            header,
            expected,
            actual,
        } => format!("invalid length for {header}: expected {expected} bytes, got {actual}"),
        AckParseError::ParseInt(header) => format!("invalid integer in {header}"),
        AckParseError::BytesMismatch { declared, actual } => {
            format!("ack byte mismatch: declared {declared}, served {actual}")
        }
        AckParseError::InvalidSignature => "invalid read acknowledgement signature".to_string(),
    };
    Response::new(StatusCode::BAD_REQUEST)
        .with_body(message.into_bytes())
        .close()
}

fn compute_client_hash(remote: &SocketAddr, domain: &str) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(domain.as_bytes());
    match remote.ip() {
        IpAddr::V4(v4) => hasher.update(&v4.octets()),
        IpAddr::V6(v6) => hasher.update(&v6.octets()),
    }
    hasher.finalize().into()
}

fn decode_hex_array<const N: usize>(
    value: &str,
    header: &'static str,
) -> Result<[u8; N], AckParseError> {
    let bytes = hex::decode(value).map_err(|_| AckParseError::Decode(header))?;
    if bytes.len() != N {
        return Err(AckParseError::Length {
            header,
            expected: N,
            actual: bytes.len(),
        });
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn decode_hex_vec(
    value: &str,
    header: &'static str,
    expected: usize,
) -> Result<Vec<u8>, AckParseError> {
    let bytes = hex::decode(value).map_err(|_| AckParseError::Decode(header))?;
    if bytes.len() != expected {
        return Err(AckParseError::Length {
            header,
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes)
}

fn parse_signed_ack(
    req: &Request<GatewayState>,
    domain: &str,
    path: &str,
    bytes: u64,
) -> Result<ReadAck, AckParseError> {
    let manifest = decode_hex_array::<32>(
        req.header(HEADER_ACK_MANIFEST)
            .ok_or(AckParseError::Missing(HEADER_ACK_MANIFEST))?,
        HEADER_ACK_MANIFEST,
    )?;
    let pk = decode_hex_array::<32>(
        req.header(HEADER_ACK_PUBKEY)
            .ok_or(AckParseError::Missing(HEADER_ACK_PUBKEY))?,
        HEADER_ACK_PUBKEY,
    )?;
    let sig = decode_hex_vec(
        req.header(HEADER_ACK_SIGNATURE)
            .ok_or(AckParseError::Missing(HEADER_ACK_SIGNATURE))?,
        HEADER_ACK_SIGNATURE,
        64,
    )?;
    let ts = req
        .header(HEADER_ACK_TIMESTAMP)
        .ok_or(AckParseError::Missing(HEADER_ACK_TIMESTAMP))?
        .parse::<u64>()
        .map_err(|_| AckParseError::ParseInt(HEADER_ACK_TIMESTAMP))?;
    let declared_bytes = req
        .header(HEADER_ACK_BYTES)
        .ok_or(AckParseError::Missing(HEADER_ACK_BYTES))?
        .parse::<u64>()
        .map_err(|_| AckParseError::ParseInt(HEADER_ACK_BYTES))?;
    if declared_bytes != bytes {
        return Err(AckParseError::BytesMismatch {
            declared: declared_bytes,
            actual: bytes,
        });
    }
    let client_hash = compute_client_hash(&req.remote_addr(), domain);
    let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
    let provider = infer_provider_for(&manifest, &path_hash, domain);
    let ack = ReadAck {
        manifest,
        path_hash,
        bytes,
        ts,
        client_hash,
        pk,
        sig,
        domain: domain.to_string(),
        provider,
        campaign_id: None,
        creative_id: None,
    };
    if ack.verify() {
        Ok(ack)
    } else {
        Err(AckParseError::InvalidSignature)
    }
}

#[cfg(not(feature = "legacy-read-acks"))]
fn build_read_ack(
    req: &Request<GatewayState>,
    state: &GatewayState,
    domain: &str,
    path: &str,
    bytes: u64,
) -> Result<ReadAck, Response> {
    parse_signed_ack(req, domain, path, bytes)
        .map(|mut ack| {
            attach_campaign_metadata(state, &mut ack);
            ack
        })
        .map_err(ack_error_response)
}

#[cfg(feature = "legacy-read-acks")]
fn build_read_ack(
    req: &Request<GatewayState>,
    state: &GatewayState,
    domain: &str,
    path: &str,
    bytes: u64,
) -> Result<ReadAck, Response> {
    let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
    match parse_signed_ack(req, domain, path, bytes) {
        Ok(mut ack) => {
            attach_campaign_metadata(state, &mut ack);
            Ok(ack)
        }
        Err(err) if err.is_missing() => Ok(ReadAck {
            manifest: [0; 32],
            path_hash,
            bytes,
            ts: now_ts(),
            client_hash: blake3::hash(domain.as_bytes()).into(),
            pk: [0u8; 32],
            sig: vec![0u8; 64],
            domain: domain.to_string(),
            provider: infer_provider_for(&[0; 32], &path_hash, domain),
            campaign_id: None,
            creative_id: None,
        }),
        Err(err) => Err(ack_error_response(err)),
    }
}

fn attach_campaign_metadata(state: &GatewayState, ack: &mut ReadAck) {
    let market = match &state.market {
        Some(handle) => handle,
        None => return,
    };
    let provider = if ack.provider.is_empty() {
        None
    } else {
        Some(ack.provider.clone())
    };
    let badges = provider
        .as_ref()
        .map(|id| service_badge::provider_badges(id))
        .unwrap_or_default();
    let ctx = ImpressionContext {
        domain: ack.domain.clone(),
        provider,
        badges,
        bytes: ack.bytes,
    };
    let key = ReservationKey {
        manifest: ack.manifest,
        path_hash: ack.path_hash,
    };
    if let Some(outcome) = market.reserve_impression(key, ctx) {
        ack.campaign_id = Some(outcome.campaign_id);
        ack.creative_id = Some(outcome.creative_id);
    }
}

fn infer_provider_for(manifest: &[u8; 32], path_hash: &[u8; 32], domain: &str) -> String {
    pipeline::provider_for_manifest(manifest, path_hash).unwrap_or_else(|| domain.to_string())
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
    market: Option<MarketplaceHandle>,
) -> diagnostics::anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let state = GatewayState {
        stake,
        read_tx,
        buckets: Arc::new(Mutex::new(HashMap::new())),
        filter: Arc::clone(&IP_FILTER),
        market,
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
            let signals = Signals::new([SIGHUP]).expect("signals");
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
    let ack = match build_read_ack(&req, &state, &domain, path, bytes) {
        Ok(ack) => ack,
        Err(response) => return Ok(response),
    };
    let _ = state.read_tx.send(ack).await;
    Ok(Response::new(StatusCode::OK).with_body(blob))
}

async fn handle_api(req: Request<GatewayState>) -> Result<Response, HttpError> {
    let state = req.state().clone();
    let domain = match state.authorize(&req) {
        Ok(host) => host,
        Err(response) => return Ok(response),
    };
    handle_func(domain, req).await
}

async fn handle_func(domain: String, req: Request<GatewayState>) -> Result<Response, HttpError> {
    let api = req.param("tail").unwrap_or("");
    if let Some(func) = lookup_dynamic(&domain, api) {
        let mut meter = wasm::GasMeter::new(func.gas_limit);
        match wasm::execute(&func.wasm, req.body_bytes(), &mut meter) {
            Ok(bytes) => Ok(Response::new(StatusCode::OK).with_body(bytes)),
            Err(err) => Ok(Response::new(StatusCode::BAD_REQUEST)
                .with_body(format!("wasm execution failed: {err}\n").into_bytes())
                .close()),
        }
    } else {
        let body = format!("dynamic endpoint '{api}' not registered\n").into_bytes();
        let _ = pipeline::fetch_wasm(&domain);
        Ok(Response::new(StatusCode::NOT_FOUND).with_body(body).close())
    }
}

#[cfg(feature = "legacy-read-acks")]
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
    use crate::storage::pipeline;
    use ad_market::{
        Campaign, CampaignTargeting, Creative, DistributionPolicy, InMemoryMarketplace,
    };
    use httpd::{Method, Router, StatusCode};
    use runtime::sync::mpsc;
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::sync::Arc;

    struct StaticStake {
        allowed: HashSet<String>,
    }

    impl StakeTable for StaticStake {
        fn has_stake(&self, domain: &str) -> bool {
            self.allowed.contains(domain)
        }
    }

    use crypto_suite::signatures::ed25519::SigningKey;
    use rand::rngs::OsRng;
    use runtime::sync::mpsc::TryRecvError;

    fn state_with_domains(domains: &[&str]) -> (GatewayState, mpsc::Receiver<ReadAck>) {
        state_with_market(domains, None)
    }

    fn state_with_market(
        domains: &[&str],
        market: Option<MarketplaceHandle>,
    ) -> (GatewayState, mpsc::Receiver<ReadAck>) {
        let allowed = domains
            .iter()
            .map(|d| d.to_string())
            .collect::<HashSet<_>>();
        let (tx, rx) = mpsc::channel(16);
        (
            GatewayState {
                stake: Arc::new(StaticStake { allowed }),
                read_tx: tx,
                buckets: Arc::new(Mutex::new(HashMap::new())),
                filter: Arc::new(Mutex::new(RateLimitFilter::new())),
                market,
            },
            rx,
        )
    }

    #[test]
    fn authorize_allows_staked_domains() {
        let (state, _rx) = state_with_domains(&["allowed.test"]);
        let router = Router::new(state.clone());
        let request = router.request_builder().host("allowed.test").build();
        let host = match state.authorize(&request) {
            Ok(host) => host,
            Err(response) => panic!("authorization failed with status {}", response.status()),
        };
        assert_eq!(host, "allowed.test");
    }

    #[test]
    fn authorize_rejects_missing_stake() {
        let (state, _rx) = state_with_domains(&[]);
        let router = Router::new(state.clone());
        let request = router.request_builder().host("unbonded.test").build();
        let response = state.authorize(&request).expect_err("missing stake");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.body(), b"domain stake required");
    }

    #[test]
    fn authorize_rate_limits_when_bucket_exhausted() {
        let (state, _rx) = state_with_domains(&["throttle.test"]);
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

        let (state, _rx) = state_with_domains(&["dyn.test"]);
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
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), 10i64.to_le_bytes());
    }

    #[test]
    fn static_read_requires_signed_ack() {
        let (state, mut rx) = state_with_domains(&["signed.test"]);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9200".parse().unwrap();
        let mut manifest = [0u8; 32];
        manifest[0] = 0xAA;
        manifest[31] = 0x55;
        let path = "/index.html";
        let bytes = 0u64;
        let ts = 1_696_969_696u64;
        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
        pipeline::override_manifest_providers_for_test(
            manifest,
            vec!["gateway-nyc-01".to_string()],
        );
        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let pk_bytes = signing.verifying_key().to_bytes();
        let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
        let client_hash = compute_client_hash(&remote, "signed.test");
        let mut hasher = Hasher::new();
        hasher.update(&manifest);
        hasher.update(&path_hash);
        hasher.update(&bytes.to_le_bytes());
        hasher.update(&ts.to_le_bytes());
        hasher.update(&client_hash);
        let message = hasher.finalize();
        let signature = signing.sign(message.as_bytes()).to_bytes();

        let request = router
            .request_builder()
            .host("signed.test")
            .path(path)
            .remote_addr(remote)
            .header(HEADER_ACK_MANIFEST, hex::encode(manifest))
            .header(HEADER_ACK_PUBKEY, hex::encode(pk_bytes))
            .header(HEADER_ACK_SIGNATURE, hex::encode(signature))
            .header(HEADER_ACK_BYTES, bytes.to_string())
            .header(HEADER_ACK_TIMESTAMP, ts.to_string())
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), Vec::<u8>::new());

        let ack = rx.try_recv().expect("ack queued");
        assert_eq!(ack.manifest, manifest);
        assert_eq!(ack.pk, pk_bytes);
        assert_eq!(ack.sig, signature.to_vec());
        assert_eq!(ack.bytes, bytes);
        assert_eq!(ack.ts, ts);
        assert_eq!(ack.client_hash, client_hash);
        assert_eq!(ack.domain, "signed.test");
        assert_eq!(ack.provider, "gateway-nyc-01");
        assert!(ack.campaign_id.is_none());
        assert!(ack.creative_id.is_none());
        pipeline::clear_test_manifest_providers();
    }

    #[test]
    fn static_read_attaches_campaign_metadata() {
        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(distribution));
        market
            .register_campaign(Campaign {
                id: "cmp1".to_string(),
                advertiser_account: "adv1".to_string(),
                budget_ct: 10_000,
                creatives: vec![Creative {
                    id: "creative1".to_string(),
                    price_per_mib_ct: 120,
                    badges: Vec::new(),
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: Vec::new(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");
        let (state, mut rx) = state_with_market(&["signed.test"], Some(market.clone()));
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9300".parse().unwrap();
        let mut manifest = [0u8; 32];
        manifest[0] = 0x11;
        let path = "/creative.html";
        let bytes = 1_048_576u64;
        let ts = 1_777_777_777u64;
        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
        pipeline::override_manifest_providers_for_test(
            manifest,
            vec!["gateway-sfo-01".to_string()],
        );
        pipeline::override_static_blob_for_test("signed.test", path, vec![0u8; bytes as usize]);
        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let pk_bytes = signing.verifying_key().to_bytes();
        let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
        let client_hash = compute_client_hash(&remote, "signed.test");
        let mut hasher = Hasher::new();
        hasher.update(&manifest);
        hasher.update(&path_hash);
        hasher.update(&bytes.to_le_bytes());
        hasher.update(&ts.to_le_bytes());
        hasher.update(&client_hash);
        let message = hasher.finalize();
        let signature = signing.sign(message.as_bytes()).to_bytes();

        let request = router
            .request_builder()
            .host("signed.test")
            .path(path)
            .remote_addr(remote)
            .header(HEADER_ACK_MANIFEST, hex::encode(manifest))
            .header(HEADER_ACK_PUBKEY, hex::encode(pk_bytes))
            .header(HEADER_ACK_SIGNATURE, hex::encode(signature))
            .header(HEADER_ACK_BYTES, bytes.to_string())
            .header(HEADER_ACK_TIMESTAMP, ts.to_string())
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let ack = rx.try_recv().expect("ack queued");
        assert_eq!(ack.campaign_id.as_deref(), Some("cmp1"));
        assert_eq!(ack.creative_id.as_deref(), Some("creative1"));
        assert_eq!(ack.provider, "gateway-sfo-01");
        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
        // The reservation should clear once the worker commits; ensure metadata persisted on ack.
    }

    #[test]
    fn static_read_matches_badge_targeted_campaign() {
        service_badge::clear_badges();
        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
        service_badge::set_physical_presence("gateway-ldn-01", true);

        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(distribution));
        market
            .register_campaign(Campaign {
                id: "cmp-badge".to_string(),
                advertiser_account: "adv-badge".to_string(),
                budget_ct: 5_000,
                creatives: vec![Creative {
                    id: "creative-badge".to_string(),
                    price_per_mib_ct: 64,
                    badges: vec!["physical_presence".to_string()],
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: vec!["physical_presence".to_string()],
                },
                metadata: HashMap::new(),
            })
            .expect("badge campaign registered");

        let (state, mut rx) = state_with_market(&["signed.test"], Some(market.clone()));
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9400".parse().unwrap();
        let manifest = [3u8; 32];
        let path = "/badge.html";
        let bytes = 1_048_576u64;
        let ts = 1_888_888_888u64;
        pipeline::override_manifest_providers_for_test(
            manifest,
            vec!["gateway-ldn-01".to_string()],
        );
        pipeline::override_static_blob_for_test("signed.test", path, vec![0u8; bytes as usize]);
        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let pk_bytes = signing.verifying_key().to_bytes();
        let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
        let client_hash = compute_client_hash(&remote, "signed.test");
        let mut hasher = Hasher::new();
        hasher.update(&manifest);
        hasher.update(&path_hash);
        hasher.update(&bytes.to_le_bytes());
        hasher.update(&ts.to_le_bytes());
        hasher.update(&client_hash);
        let signature = signing.sign(hasher.finalize().as_bytes()).to_bytes();

        let request = router
            .request_builder()
            .host("signed.test")
            .path(path)
            .remote_addr(remote)
            .header(HEADER_ACK_MANIFEST, hex::encode(manifest))
            .header(HEADER_ACK_PUBKEY, hex::encode(pk_bytes))
            .header(HEADER_ACK_SIGNATURE, hex::encode(signature))
            .header(HEADER_ACK_BYTES, bytes.to_string())
            .header(HEADER_ACK_TIMESTAMP, ts.to_string())
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let ack = rx.try_recv().expect("ack queued");
        assert_eq!(ack.campaign_id.as_deref(), Some("cmp-badge"));
        assert_eq!(ack.creative_id.as_deref(), Some("creative-badge"));
        assert_eq!(ack.provider, "gateway-ldn-01");

        service_badge::clear_badges();
        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
    }

    #[test]
    fn static_read_selects_provider_from_multiple_candidates() {
        pipeline::clear_test_manifest_providers();

        let manifest = [0x42u8; 32];
        let providers = vec![
            "gateway-ams-01".to_string(),
            "gateway-ldn-01".to_string(),
            "gateway-nyc-01".to_string(),
        ];
        pipeline::override_manifest_providers_for_test(manifest, providers);

        let (state, mut rx) = state_with_domains(&["multi.test"]);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);

        let mut rng = OsRng::default();
        let signing = SigningKey::generate(&mut rng);
        let pk_bytes = signing.verifying_key().to_bytes();
        let remote: SocketAddr = "127.0.0.1:9500".parse().unwrap();

        let mut send_signed = |path: &str, ts: u64, expected: &str| -> String {
            let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
            let computed =
                pipeline::provider_for_manifest(&manifest, &path_hash).expect("provider available");
            assert_eq!(
                computed, expected,
                "provider selection changed unexpectedly"
            );
            let bytes = 256u64;
            pipeline::override_static_blob_for_test("multi.test", path, vec![0u8; bytes as usize]);
            let client_hash = compute_client_hash(&remote, "multi.test");
            let mut hasher = Hasher::new();
            hasher.update(&manifest);
            hasher.update(&path_hash);
            hasher.update(&bytes.to_le_bytes());
            hasher.update(&ts.to_le_bytes());
            hasher.update(&client_hash);
            let signature = signing.sign(hasher.finalize().as_bytes()).to_bytes();

            let request = router
                .request_builder()
                .host("multi.test")
                .path(path)
                .remote_addr(remote)
                .header(HEADER_ACK_MANIFEST, hex::encode(manifest))
                .header(HEADER_ACK_PUBKEY, hex::encode(pk_bytes))
                .header(HEADER_ACK_SIGNATURE, hex::encode(signature))
                .header(HEADER_ACK_BYTES, bytes.to_string())
                .header(HEADER_ACK_TIMESTAMP, ts.to_string())
                .build();
            let response = runtime::block_on(router.handle(request)).unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let ack = rx.try_recv().expect("ack queued");
            assert_eq!(ack.provider, expected);
            assert_eq!(ack.manifest, manifest);
            assert_eq!(ack.pk, pk_bytes);
            assert_eq!(ack.bytes, bytes);
            ack.provider
        };

        let mut expected = Vec::new();
        let mut unique = BTreeSet::new();
        for path in [
            "/multi/first",
            "/multi/second",
            "/multi/third",
            "/multi/fourth",
            "/multi/fifth",
        ] {
            let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
            let provider =
                pipeline::provider_for_manifest(&manifest, &path_hash).expect("provider available");
            unique.insert(provider.clone());
            expected.push((path, provider));
            if unique.len() > 1 {
                break;
            }
        }
        assert!(
            unique.len() > 1,
            "expected at least two providers to be selected"
        );

        for (idx, (path, provider)) in expected.into_iter().enumerate() {
            let observed = send_signed(path, 1_700_000_001 + idx as u64, &provider);
            assert_eq!(observed, provider);
        }

        pipeline::clear_test_manifest_providers();
        pipeline::clear_test_static_blobs();
    }

    #[test]
    fn static_read_rejects_missing_ack_headers() {
        let (state, mut rx) = state_with_domains(&["unsigned.test"]);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let request = router
            .request_builder()
            .host("unsigned.test")
            .path("/file.txt")
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(response.body()).contains("missing"));
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            other => panic!("unexpected ack state: {other:?}"),
        }
    }
}
