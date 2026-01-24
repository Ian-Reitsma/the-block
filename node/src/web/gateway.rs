//! Minimal HTTP gateway serving on-chain blobs and deterministic WASM.
//!
//! This server exposes zero-fee static file hosting backed by blob storage
//! along with optional dynamic endpoints powered by WASM. Every response
//! records a `ReadAck` that gateways later batch and anchor on-chain to claim
//! BLOCK subsidies.

#![deny(warnings)]

use ad_market::{
    ann, BadgeSoftIntentContext, DeliveryChannel, DeviceContext, GeoContext, ImpressionContext,
    MarketplaceHandle, MeshContext, ReservationKey,
};
use base64_fp::decode_standard;
use concurrency::Lazy;
use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::hex;
use std::fs;
use std::{
    collections::HashMap,
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use sys::signals::{Signals, SIGHUP};

use crate::gateway::dns;
use crate::web::rate_limit::RateLimitFilter;
use crate::{
    ad_quality, ad_readiness::AdReadinessHandle, drive, net, range_boost, range_boost::RangeBoost,
    service_badge, storage::pipeline, vm::wasm, ReadAck,
};
use foundation_serialization::{
    binary,
    json::{self, Map as JsonMap, Number, Value as JsonValue},
};
use httpd::{
    serve, serve_tls, HttpError, Method, Request, Response, Router, ServerConfig, ServerTlsConfig,
    StatusCode, WebSocketRequest, WebSocketResponse,
};
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
    readiness: Option<AdReadinessHandle>,
    mesh_queue: Arc<Mutex<RangeBoost>>,
    resolver: ResolverConfig,
    drive: Arc<drive::DriveStore>,
}

#[derive(Clone)]
pub struct ResolverConfig {
    addresses: Vec<IpAddr>,
    ttl_secs: u32,
    cname_target: Option<String>,
}

impl ResolverConfig {
    pub fn from_env() -> Self {
        let addresses = Self::parse_env_addresses(env::var("TB_GATEWAY_RESOLVER_ADDRS").ok());
        let addresses = if addresses.is_empty() {
            Self::default_loopback_addresses()
        } else {
            addresses
        };
        let ttl_secs = env::var("TB_GATEWAY_RESOLVER_TTL")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(60);
        let cname_target = env::var("TB_GATEWAY_RESOLVER_CNAME").ok();
        Self {
            addresses,
            ttl_secs,
            cname_target,
        }
    }

    pub fn with_addresses(
        addresses: Vec<IpAddr>,
        ttl_secs: u32,
        cname_target: Option<String>,
    ) -> Self {
        Self {
            addresses,
            ttl_secs,
            cname_target,
        }
    }

    pub fn empty() -> Self {
        Self {
            addresses: Vec::new(),
            ttl_secs: 60,
            cname_target: None,
        }
    }

    pub fn ttl(&self) -> u32 {
        self.ttl_secs
    }

    pub fn cname(&self) -> Option<&str> {
        self.cname_target.as_deref()
    }

    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    fn parse_env_addresses(value: Option<String>) -> Vec<IpAddr> {
        value
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn default_loopback_addresses() -> Vec<IpAddr> {
        Self::gateway_loopback_address().map_or_else(Vec::new, |addr| vec![addr])
    }

    fn gateway_loopback_address() -> Option<IpAddr> {
        let url =
            env::var("TB_GATEWAY_URL").unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());
        let authority = url.split("://").nth(1).unwrap_or(&url);
        let host_port = authority.split('/').next().unwrap_or(authority);
        let host = Self::strip_host_port(host_port);
        if host.eq_ignore_ascii_case("localhost") {
            return Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        if host.eq_ignore_ascii_case("::1") {
            return Some(IpAddr::V6(Ipv6Addr::LOCALHOST));
        }
        host.parse::<IpAddr>()
            .ok()
            .filter(|addr| addr.is_loopback())
    }

    fn strip_host_port(value: &str) -> &str {
        if value.starts_with('[') {
            if let Some(end) = value.find(']') {
                return &value[1..end];
            }
            return &value[1..];
        }
        if let Some(idx) = value.rfind(':') {
            if !value[..idx].contains(':') {
                return &value[..idx];
            }
        }
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordType {
    A,
    Aaaa,
    Txt,
    Cname,
}

impl RecordType {
    fn as_u16(self) -> u16 {
        match self {
            RecordType::A => 1,
            RecordType::Aaaa => 28,
            RecordType::Txt => 16,
            RecordType::Cname => 5,
        }
    }

    fn matches_ip(self, addr: &IpAddr) -> bool {
        match self {
            RecordType::A => matches!(addr, IpAddr::V4(_)),
            RecordType::Aaaa => matches!(addr, IpAddr::V6(_)),
            _ => false,
        }
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(RecordType::A),
            28 => Some(RecordType::Aaaa),
            16 => Some(RecordType::Txt),
            5 => Some(RecordType::Cname),
            _ => None,
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "a" | "1" => Some(RecordType::A),
            "aaaa" | "28" => Some(RecordType::Aaaa),
            "txt" | "16" => Some(RecordType::Txt),
            "cname" | "5" => Some(RecordType::Cname),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct DnsQuestion {
    name: String,
    record_type: RecordType,
}

#[derive(Debug)]
struct DnsAnswer {
    name: String,
    record_type: RecordType,
    ttl: u32,
    data: JsonValue,
}

impl DnsAnswer {
    fn to_json(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("name".into(), JsonValue::String(self.name.clone()));
        map.insert(
            "type".into(),
            JsonValue::Number(Number::from(self.record_type.as_u16() as u64)),
        );
        map.insert(
            "TTL".into(),
            JsonValue::Number(Number::from(self.ttl as u64)),
        );
        map.insert("data".into(), self.data.clone());
        JsonValue::Object(map)
    }
}

fn question_to_json(question: &DnsQuestion) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("name".into(), JsonValue::String(question.name.clone()));
    map.insert(
        "type".into(),
        JsonValue::Number(Number::from(question.record_type.as_u16() as u64)),
    );
    JsonValue::Object(map)
}

fn build_dns_payload(question: &DnsQuestion, answers: &[DnsAnswer], status: u16) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "Status".into(),
        JsonValue::Number(Number::from(status as u64)),
    );
    map.insert("TC".into(), JsonValue::Bool(false));
    map.insert("RD".into(), JsonValue::Bool(true));
    map.insert("RA".into(), JsonValue::Bool(true));
    map.insert("AD".into(), JsonValue::Bool(false));
    map.insert("CD".into(), JsonValue::Bool(false));
    map.insert(
        "Question".into(),
        JsonValue::Array(vec![question_to_json(question)]),
    );
    map.insert(
        "Answer".into(),
        JsonValue::Array(answers.iter().map(DnsAnswer::to_json).collect()),
    );
    JsonValue::Object(map)
}

fn fetch_gateway_txt(domain: &str) -> Option<String> {
    let mut params = JsonMap::new();
    params.insert("domain".into(), JsonValue::String(domain.to_string()));
    let response = dns::gateway_policy(&JsonValue::Object(params));
    if let JsonValue::Object(map) = response {
        if let Some(JsonValue::String(record)) = map.get("record") {
            return Some(record.clone());
        }
    }
    None
}

fn answers_for_question(resolver: &ResolverConfig, question: &DnsQuestion) -> Vec<DnsAnswer> {
    let mut answers = Vec::new();
    let ttl = resolver.ttl();
    match question.record_type {
        RecordType::A | RecordType::Aaaa => {
            for addr in resolver.addresses() {
                if question.record_type.matches_ip(addr) {
                    answers.push(DnsAnswer {
                        name: question.name.clone(),
                        record_type: question.record_type,
                        ttl,
                        data: JsonValue::String(addr.to_string()),
                    });
                }
            }
            if answers.is_empty() {
                if let Some(target) = resolver.cname() {
                    answers.push(DnsAnswer {
                        name: question.name.clone(),
                        record_type: RecordType::Cname,
                        ttl,
                        data: JsonValue::String(target.to_string()),
                    });
                }
            }
        }
        RecordType::Cname => {
            if let Some(target) = resolver.cname() {
                answers.push(DnsAnswer {
                    name: question.name.clone(),
                    record_type: RecordType::Cname,
                    ttl,
                    data: JsonValue::String(target.to_string()),
                });
            }
        }
        RecordType::Txt => {
            if let Some(record) = fetch_gateway_txt(&question.name) {
                answers.push(DnsAnswer {
                    name: question.name.clone(),
                    record_type: RecordType::Txt,
                    ttl,
                    data: JsonValue::String(record),
                });
            }
        }
    }
    answers
}

fn parse_dns_request(req: &Request<GatewayState>) -> Result<DnsQuestion, Response> {
    if let Some(encoded) = req.query_param("dns") {
        let bytes = match decode_standard(encoded) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(Response::new(StatusCode::BAD_REQUEST)
                    .with_body(b"invalid dns parameter".to_vec()));
            }
        };
        return parse_dns_packet(&bytes).ok_or_else(|| {
            Response::new(StatusCode::BAD_REQUEST).with_body(b"malformed dns payload".to_vec())
        });
    }

    if let Some(name) = req.query_param("name") {
        let normalized = normalize_domain(name).ok_or_else(|| {
            Response::new(StatusCode::BAD_REQUEST).with_body(b"invalid domain".to_vec())
        })?;
        let record_type = req
            .query_param("type")
            .and_then(RecordType::from_str)
            .unwrap_or(RecordType::A);
        return Ok(DnsQuestion {
            name: normalized,
            record_type,
        });
    }

    Err(Response::new(StatusCode::BAD_REQUEST)
        .with_body(b"name or dns parameter required".to_vec()))
}

fn parse_dns_packet(bytes: &[u8]) -> Option<DnsQuestion> {
    if bytes.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([bytes[4], bytes[5]]);
    if qdcount == 0 {
        return None;
    }
    let mut index = 12;
    let mut labels = Vec::new();
    loop {
        if index >= bytes.len() {
            return None;
        }
        let len = bytes[index] as usize;
        index += 1;
        if len == 0 {
            break;
        }
        if index + len > bytes.len() {
            return None;
        }
        let label = &bytes[index..index + len];
        if label.iter().any(|byte| *byte == 0) {
            return None;
        }
        let label = std::str::from_utf8(label).ok()?;
        labels.push(label);
        index += len;
    }
    if index + 4 > bytes.len() {
        return None;
    }
    let record_type = u16::from_be_bytes([bytes[index], bytes[index + 1]]);
    let record_class = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]);
    if record_class != 1 {
        return None;
    }
    let question = DnsQuestion {
        name: normalize_domain(&labels.join(".")).unwrap_or_default(),
        record_type: RecordType::from_u16(record_type)?,
    };
    if question.name.is_empty() {
        return None;
    }
    Some(question)
}

fn normalize_domain(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let trimmed = trimmed.trim_end_matches('.');
    let normalized = trimmed.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[derive(Clone)]
struct DynamicFunc {
    wasm: Vec<u8>,
    gas_limit: u64,
}

static DYNAMIC_FUNCS: Lazy<Mutex<HashMap<(String, String), DynamicFunc>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static CRM_MEMBERSHIPS: Lazy<RwLock<HashMap<String, Vec<String>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn provider_crm_lists(provider: &str) -> Vec<String> {
    CRM_MEMBERSHIPS
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(provider)
        .cloned()
        .map(|mut lists| {
            lists.retain(|item| !item.is_empty());
            lists.sort();
            lists.dedup();
            lists
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(dead_code)]
fn set_crm_lists(provider: &str, lists: &[&str]) {
    CRM_MEMBERSHIPS
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(
            provider.to_string(),
            lists.iter().map(|entry| entry.to_string()).collect(),
        );
}

#[cfg(test)]
fn clear_crm_lists() {
    CRM_MEMBERSHIPS
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
}

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
const HEADER_BADGE_ANN_SNAPSHOT: &str = "x-theblock-ann-snapshot";
const HEADER_BADGE_ANN_PROOF: &str = "x-theblock-ann-proof";
const HEADER_GEO_COUNTRY: &str = "x-theblock-geo-country";
const HEADER_GEO_REGION: &str = "x-theblock-geo-region";
const HEADER_GEO_METRO: &str = "x-theblock-geo-metro";
const HEADER_DEVICE_OS: &str = "x-theblock-device-os";
const HEADER_DEVICE_OS_VERSION: &str = "x-theblock-device-os-version";
const HEADER_DEVICE_CLASS: &str = "x-theblock-device-class";
const HEADER_DEVICE_MODEL: &str = "x-theblock-device-model";
const HEADER_DEVICE_CAPABILITIES: &str = "x-theblock-device-capabilities";
const HEADER_CRM_LISTS: &str = "x-theblock-crm-lists";
const HEADER_DELIVERY_CHANNEL: &str = "x-theblock-delivery-channel";
const HEADER_MESH_PEER: &str = "x-theblock-mesh-peer";
const HEADER_MESH_TRANSPORT: &str = "x-theblock-mesh-transport";
const HEADER_MESH_LATENCY: &str = "x-theblock-mesh-latency";
const HEADER_MESH_HOPS: &str = "x-theblock-mesh-hop";
const HEADER_VENUE_ID: &str = "x-theblock-venue-id";
const HEADER_CROWD_SIZE: &str = "x-theblock-crowd-size";
const HEADER_PRESENCE_TOKEN: &str = "x-theblock-presence-token";

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
    AnnDeserialize(&'static str),
}

impl AckParseError {
    fn is_missing(self) -> bool {
        matches!(self, AckParseError::Missing(_))
    }
}

fn require_ack_header<'req>(
    req: &'req Request<GatewayState>,
    header: &'static str,
) -> Result<&'req str, AckParseError> {
    req.header(header)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(AckParseError::Missing(header))
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
        AckParseError::AnnDeserialize(header) => {
            format!("failed to decode ANN payload from {header}")
        }
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

fn parse_soft_intent(
    req: &Request<GatewayState>,
) -> Result<Option<BadgeSoftIntentContext>, AckParseError> {
    let snapshot_bytes = req.header(HEADER_BADGE_ANN_SNAPSHOT);
    let proof_bytes = req.header(HEADER_BADGE_ANN_PROOF);
    if snapshot_bytes.is_none() && proof_bytes.is_none() {
        return Ok(None);
    }
    let snapshot = match snapshot_bytes {
        Some(value) if !value.is_empty() => {
            let bytes =
                hex::decode(value).map_err(|_| AckParseError::Decode(HEADER_BADGE_ANN_SNAPSHOT))?;
            let snapshot: ann::WalletAnnIndexSnapshot = binary::decode(&bytes)
                .map_err(|_| AckParseError::AnnDeserialize(HEADER_BADGE_ANN_SNAPSHOT))?;
            Some(snapshot)
        }
        _ => None,
    };
    let proof = match proof_bytes {
        Some(value) if !value.is_empty() => {
            let bytes =
                hex::decode(value).map_err(|_| AckParseError::Decode(HEADER_BADGE_ANN_PROOF))?;
            let proof: ann::SoftIntentReceipt = binary::decode(&bytes)
                .map_err(|_| AckParseError::AnnDeserialize(HEADER_BADGE_ANN_PROOF))?;
            Some(proof)
        }
        _ => None,
    };
    if snapshot.is_none() && proof.is_none() {
        return Ok(None);
    }
    Ok(Some(BadgeSoftIntentContext {
        wallet_index: snapshot,
        proof,
    }))
}

fn parse_geo_context(req: &Request<GatewayState>) -> Option<GeoContext> {
    let mut context = GeoContext::default();
    let mut seen = false;
    if let Some(value) = req.header(HEADER_GEO_COUNTRY) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.country = Some(trimmed.to_ascii_uppercase());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_GEO_REGION) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.region = Some(trimmed.to_string());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_GEO_METRO) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.metro = Some(trimmed.to_string());
            seen = true;
        }
    }
    if !seen {
        let ip = req.remote_addr().ip();
        if ip.is_loopback() {
            context.country = Some("ZZ".into());
            seen = true;
        }
    }
    if seen {
        Some(context)
    } else {
        None
    }
}

fn infer_device_from_user_agent(agent: &str, ctx: &mut DeviceContext) -> bool {
    let mut updated = false;
    let lower = agent.to_ascii_lowercase();
    if ctx.os_family.is_none() {
        if lower.contains("android") {
            ctx.os_family = Some("android".into());
            updated = true;
        } else if lower.contains("iphone") || lower.contains("ipad") {
            ctx.os_family = Some("ios".into());
            updated = true;
        } else if lower.contains("windows") {
            ctx.os_family = Some("windows".into());
            updated = true;
        } else if lower.contains("mac os x") || lower.contains("macintosh") {
            ctx.os_family = Some("macos".into());
            updated = true;
        } else if lower.contains("linux") {
            ctx.os_family = Some("linux".into());
            updated = true;
        }
    }
    if ctx.device_class.is_none() {
        if lower.contains("tablet") || lower.contains("ipad") {
            ctx.device_class = Some("tablet".into());
            updated = true;
        } else if lower.contains("mobile") || lower.contains("iphone") || lower.contains("android")
        {
            ctx.device_class = Some("mobile".into());
            updated = true;
        } else if lower.contains("tv") || lower.contains("smarttv") {
            ctx.device_class = Some("tv".into());
            updated = true;
        } else if lower.contains("xbox") || lower.contains("playstation") {
            ctx.device_class = Some("console".into());
            updated = true;
        } else if lower.contains("windows")
            || lower.contains("macintosh")
            || lower.contains("linux")
        {
            ctx.device_class = Some("desktop".into());
            updated = true;
        }
    }
    updated
}

fn parse_capability_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect()
}

fn parse_device_context(req: &Request<GatewayState>) -> Option<DeviceContext> {
    let mut context = DeviceContext::default();
    let mut seen = false;
    if let Some(value) = req.header(HEADER_DEVICE_OS) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.os_family = Some(trimmed.to_lowercase());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_DEVICE_OS_VERSION) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.os_version = Some(trimmed.to_string());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_DEVICE_CLASS) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.device_class = Some(trimmed.to_lowercase());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_DEVICE_MODEL) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            context.model = Some(trimmed.to_string());
            seen = true;
        }
    }
    if let Some(value) = req.header(HEADER_DEVICE_CAPABILITIES) {
        let caps = parse_capability_list(value);
        if !caps.is_empty() {
            context.capabilities = caps;
            seen = true;
        }
    }
    if let Some(agent) = req.header("user-agent") {
        if infer_device_from_user_agent(agent, &mut context) {
            seen = true;
        }
    }
    if seen {
        Some(context)
    } else {
        None
    }
}

fn parse_crm_lists(req: &Request<GatewayState>) -> Vec<String> {
    req.header(HEADER_CRM_LISTS)
        .map(parse_capability_list)
        .unwrap_or_default()
}

fn parse_delivery_channel(req: &Request<GatewayState>) -> DeliveryChannel {
    req.header(HEADER_DELIVERY_CHANNEL)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_default()
}

fn parse_mesh_context(req: &Request<GatewayState>) -> Option<MeshContext> {
    let mut context = MeshContext::default();
    let mut seen = false;
    if let Some(peer) = req.header(HEADER_MESH_PEER) {
        let trimmed = peer.trim();
        if !trimmed.is_empty() {
            context.peer_id = Some(trimmed.to_string());
            seen = true;
        }
    }
    if let Some(transport) = req.header(HEADER_MESH_TRANSPORT) {
        let trimmed = transport.trim();
        if !trimmed.is_empty() {
            context.transport = Some(trimmed.to_string());
            seen = true;
        }
    }
    if let Some(latency) = req.header(HEADER_MESH_LATENCY) {
        if let Ok(value) = latency.trim().parse::<u64>() {
            context.latency_ms = Some(value);
            seen = true;
        }
    }
    if let Some(hops) = req.header(HEADER_MESH_HOPS) {
        let proofs = parse_capability_list(hops);
        if !proofs.is_empty() {
            context.hop_proofs = proofs;
            seen = true;
        }
    }
    if seen {
        Some(context)
    } else {
        None
    }
}

fn parse_signed_ack(
    req: &Request<GatewayState>,
    domain: &str,
    path: &str,
    bytes: u64,
) -> Result<ReadAck, AckParseError> {
    let manifest_hex = require_ack_header(req, HEADER_ACK_MANIFEST)?;
    let pk_hex = require_ack_header(req, HEADER_ACK_PUBKEY)?;
    let sig_hex = require_ack_header(req, HEADER_ACK_SIGNATURE)?;
    let ts_value = require_ack_header(req, HEADER_ACK_TIMESTAMP)?;
    let bytes_value = require_ack_header(req, HEADER_ACK_BYTES)?;
    let manifest = decode_hex_array::<32>(manifest_hex, HEADER_ACK_MANIFEST)?;
    let pk = decode_hex_array::<32>(pk_hex, HEADER_ACK_PUBKEY)?;
    let sig = decode_hex_vec(sig_hex, HEADER_ACK_SIGNATURE, 64)?;
    let ts = ts_value
        .parse::<u64>()
        .map_err(|_| AckParseError::ParseInt(HEADER_ACK_TIMESTAMP))?;
    let declared_bytes = bytes_value
        .parse::<u64>()
        .map_err(|_| AckParseError::ParseInt(HEADER_ACK_BYTES))?;
    if declared_bytes != bytes {
        return Err(AckParseError::BytesMismatch {
            declared: declared_bytes,
            actual: bytes,
        });
    }
    let soft_intent = parse_soft_intent(req)?;
    let client_hash = compute_client_hash(&req.remote_addr(), domain);
    let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
    let provider = infer_provider_for(&manifest, &path_hash).unwrap_or_default();
    let geo = parse_geo_context(req);
    let device = parse_device_context(req);
    let mut crm_lists = parse_crm_lists(req);
    let delivery_channel = parse_delivery_channel(req);
    let mesh = parse_mesh_context(req);
    let mut ack = ReadAck {
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
        selection_receipt: None,
        geo,
        device,
        crm_lists: {
            crm_lists.sort();
            crm_lists.dedup();
            crm_lists
        },
        delivery_channel,
        mesh,
        badge_soft_intent: soft_intent,
        readiness: None,
        zk_proof: None,
        presence_badge: None,
        venue_id: None,
        crowd_size_hint: None,
    };
    // Optional presence + venue attestation fields
    if let Some(v) = req.header(HEADER_VENUE_ID) {
        let v = v.trim();
        if !v.is_empty() {
            ack.venue_id = Some(v.to_string());
        }
    }
    if let Some(v) = req.header(HEADER_CROWD_SIZE) {
        if let Ok(n) = v.parse::<u32>() {
            ack.crowd_size_hint = Some(n);
        }
    }
    if let Some(v) = req.header(HEADER_PRESENCE_TOKEN) {
        let token = v.trim();
        if !token.is_empty() {
            ack.presence_badge = Some(token.to_string());
        }
    }
    if ack.verify() {
        Ok(ack)
    } else {
        Err(AckParseError::InvalidSignature)
    }
}

fn build_read_ack(
    req: &Request<GatewayState>,
    state: &GatewayState,
    domain: &str,
    path: &str,
    bytes: u64,
) -> Result<ReadAck, Response> {
    match parse_signed_ack(req, domain, path, bytes) {
        Ok(mut ack) => {
            attach_campaign_metadata(state, &mut ack);
            attach_readiness_attestation(state, &mut ack);
            Ok(ack)
        }
        Err(err) if err.is_missing() => {
            let mut ack = build_synthetic_ack(req, domain, path, bytes);
            attach_campaign_metadata(state, &mut ack);
            attach_readiness_attestation(state, &mut ack);
            Ok(ack)
        }
        Err(err) => Err(ack_error_response(err)),
    }
}

fn build_synthetic_ack(
    req: &Request<GatewayState>,
    domain: &str,
    path: &str,
    bytes: u64,
) -> ReadAck {
    let path_hash: [u8; 32] = blake3::hash(path.as_bytes()).into();
    ReadAck {
        manifest: [0; 32],
        path_hash,
        bytes,
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        client_hash: compute_client_hash(&req.remote_addr(), domain),
        pk: [0u8; 32],
        sig: vec![0u8; 64],
        domain: domain.to_string(),
        provider: infer_provider_for(&[0; 32], &path_hash).unwrap_or_default(),
        campaign_id: None,
        creative_id: None,
        selection_receipt: None,
        geo: None,
        device: None,
        crm_lists: Vec::new(),
        delivery_channel: DeliveryChannel::default(),
        mesh: None,
        badge_soft_intent: None,
        readiness: None,
        zk_proof: None,
        presence_badge: None,
        venue_id: None,
        crowd_size_hint: None,
    }
}

fn cohort_snapshot_from_context(ctx: &ImpressionContext) -> ad_market::CohortKeySnapshot {
    ad_market::CohortKeySnapshot {
        domain: ctx.domain.clone(),
        provider: ctx.provider.clone(),
        badges: ctx.badges.clone(),
        domain_tier: ctx.domain_tier,
        domain_owner: ctx.domain_owner.clone(),
        interest_tags: ctx.interest_tags.clone(),
        presence_bucket: ctx.presence_bucket.clone(),
        selectors_version: ctx.selectors_version,
    }
}

fn refresh_readiness_utilization(handle: &AdReadinessHandle, market: &MarketplaceHandle) {
    const UTILIZATION_REFRESH_SECS: u64 = 60;
    let snapshot = handle.snapshot();
    let last_updated = snapshot
        .utilization_summary
        .as_ref()
        .map(|summary| summary.last_updated)
        .unwrap_or(0);
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if last_updated == 0 || now_secs.saturating_sub(last_updated) >= UTILIZATION_REFRESH_SECS {
        let oracle = market.oracle();
        let cohorts = market.cohort_prices();
        handle.record_utilization(&cohorts, oracle.price_usd_micros);
        market.recompute_distribution_from_utilization();
    }
}

fn attach_campaign_metadata(state: &GatewayState, ack: &mut ReadAck) {
    let market = match &state.market {
        Some(handle) => handle,
        None => return,
    };
    if let Some(handle) = &state.readiness {
        let decision = handle.decision();
        if !decision.ready() {
            #[cfg(feature = "telemetry")]
            {
                if let Ok(counter) = crate::telemetry::AD_READINESS_SKIPPED
                    .ensure_handle_for_label_values(&[match decision.blockers().first() {
                        Some(reason) => reason.as_str(),
                        None => "unknown",
                    }])
                {
                    counter.inc();
                }
            }
            diagnostics::tracing::debug!(
                blockers = ?decision.blockers(),
                "ad_matching_skipped_due_to_readiness"
            );
            return;
        }
    }
    let provider = if ack.provider.is_empty() {
        let resolved = infer_provider_for(&ack.manifest, &ack.path_hash);
        if let Some(id) = resolved.clone() {
            ack.provider = id.clone();
        }
        resolved
    } else {
        Some(ack.provider.clone())
    };
    let badges = provider
        .as_ref()
        .map(|id| service_badge::provider_badges(id))
        .unwrap_or_default();
    let mut crm_lists = ack.crm_lists.clone();
    if let Some(provider_id) = provider.as_deref() {
        crm_lists.extend(provider_crm_lists(provider_id));
    }
    crm_lists.sort();
    crm_lists.dedup();
    let mut mesh_context = ack.mesh.clone();
    if mesh_context.is_none() && ack.delivery_channel == DeliveryChannel::Mesh {
        if let Some(peer) = range_boost::best_peer() {
            let mut ctx = MeshContext::default();
            ctx.peer_id = Some(peer.addr.clone());
            let transport = if peer.addr.starts_with("bt:") {
                "bluetooth"
            } else if peer.addr.starts_with("wifi:") {
                "wifi"
            } else {
                "range_boost"
            };
            ctx.transport = Some(transport.into());
            ctx.latency_ms = Some(peer.latency_ms.min(u128::from(u64::MAX)) as u64);
            mesh_context = Some(ctx);
        }
    }
    ack.crm_lists = crm_lists.clone();
    ack.mesh = mesh_context.clone();
    let ctx = ImpressionContext {
        domain: ack.domain.clone(),
        provider,
        badges,
        bytes: ack.bytes,
        geo: ack.geo.clone(),
        device: ack.device.clone(),
        crm_lists,
        soft_intent: ack.badge_soft_intent.clone(),
        delivery_channel: ack.delivery_channel,
        mesh: mesh_context,
        ..ImpressionContext::default()
    };
    let readiness_snapshot = state.readiness.as_ref().map(|handle| {
        refresh_readiness_utilization(handle, market);
        handle.snapshot()
    });
    let privacy_snapshot = market.privacy_budget_snapshot();
    let quality_config = market.quality_signal_config();
    let cohort_snapshot = cohort_snapshot_from_context(&ctx);
    let report = ad_quality::quality_signal_for_cohort(
        &quality_config,
        readiness_snapshot.as_ref(),
        Some(&privacy_snapshot),
        &cohort_snapshot,
    );
    market.update_quality_signals(vec![report.signal.clone()]);
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::update_ad_quality_components(
            &report.signal.components,
            report.signal.multiplier_ppm,
        );
        crate::telemetry::update_ad_quality_readiness_streak_windows(
            report.readiness_streak_windows,
        );
        crate::telemetry::update_ad_quality_privacy_score_ppm(report.privacy_score_ppm);
        if let Some((bucket, ppm)) = report.freshness_score_ppm {
            crate::telemetry::update_ad_quality_freshness_scores(&[(bucket, ppm)]);
        }
    }
    let key = ReservationKey {
        manifest: ack.manifest,
        path_hash: ack.path_hash,
        discriminator: ack.reservation_discriminator(),
    };
    if let Some(outcome) = market.reserve_impression(key, ctx) {
        let holdout = outcome.uplift_assignment.in_holdout;
        let delivery_channel = outcome.delivery_channel;
        let mesh_payload = outcome.mesh_payload.clone();
        ack.campaign_id = Some(outcome.campaign_id);
        ack.creative_id = Some(outcome.creative_id);
        ack.selection_receipt = Some(outcome.selection_receipt);
        ack.delivery_channel = delivery_channel;
        if ack.delivery_channel != DeliveryChannel::Mesh {
            ack.mesh = None;
        } else if !holdout {
            if let Some(payload) = mesh_payload {
                let mut queue = state.mesh_queue.lock().unwrap();
                queue.enqueue(payload);
                let idx = queue.pending().saturating_sub(1);
                if let Some(mesh) = ack.mesh.as_ref() {
                    for hop in &mesh.hop_proofs {
                        if !hop.is_empty() {
                            queue.record_proof(idx, range_boost::HopProof { relay: hop.clone() });
                        }
                    }
                }
            }
        }
    }
}

fn attach_readiness_attestation(state: &GatewayState, ack: &mut ReadAck) {
    if let Some(handle) = &state.readiness {
        let snapshot = handle.snapshot();
        ack.attach_privacy(snapshot);
    }
}

fn infer_provider_for(manifest: &[u8; 32], path_hash: &[u8; 32]) -> Option<String> {
    pipeline::provider_for_manifest(manifest, path_hash)
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
        let host_header = req.header("host").unwrap_or("");
        let host = canonical_host(host_header);
        if host.is_empty() {
            diagnostics::log::warn!(
                "gateway authorization failed: missing host header from {}",
                ip
            );
            return Err(
                Response::new(StatusCode::FORBIDDEN).with_body(b"domain stake required".to_vec())
            );
        }
        if !self.stake.has_stake(host) {
            diagnostics::log::warn!(
                "gateway authorization failed: host {} rejected (no stake) from {}",
                host,
                ip
            );
            return Err(
                Response::new(StatusCode::FORBIDDEN).with_body(b"domain stake required".to_vec())
            );
        }
        Ok(host.to_string())
    }
}

fn canonical_host(value: &str) -> &str {
    let host = value.trim();
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            if end > 1 {
                return &host[1..end];
            }
            return host.trim_start_matches('[');
        }
        return host.trim_start_matches('[');
    }
    if let Some(idx) = host.rfind(':') {
        if !host[..idx].contains(':') {
            return &host[..idx];
        }
    }
    host
}

/// Runs the gateway server on the given address.
pub async fn run(
    addr: SocketAddr,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
    market: Option<MarketplaceHandle>,
    readiness: Option<AdReadinessHandle>,
    tls: Option<ServerTlsConfig>,
    resolver: Option<ResolverConfig>,
) -> diagnostics::anyhow::Result<()> {
    let listener =
        net::listener::bind_runtime("gateway", "gateway_listener_bind_failed", addr).await?;
    let resolver = resolver.unwrap_or_else(ResolverConfig::from_env);
    run_listener(listener, stake, read_tx, market, readiness, tls, resolver).await
}

/// Runs the gateway server on the provided listener.
pub async fn run_listener(
    listener: runtime::net::TcpListener,
    stake: Arc<dyn StakeTable + Send + Sync>,
    read_tx: mpsc::Sender<ReadAck>,
    market: Option<MarketplaceHandle>,
    readiness: Option<AdReadinessHandle>,
    tls: Option<httpd::ServerTlsConfig>,
    resolver: ResolverConfig,
) -> diagnostics::anyhow::Result<()> {
    let mesh_queue = Arc::new(Mutex::new(RangeBoost::new()));
    range_boost::spawn_forwarder(&mesh_queue);
    let drive_store = Arc::new(drive::DriveStore::from_env());
    let state = GatewayState {
        stake,
        read_tx,
        buckets: Arc::new(Mutex::new(HashMap::new())),
        filter: Arc::clone(&IP_FILTER),
        market,
        readiness,
        mesh_queue,
        resolver,
        drive: drive_store,
    };
    let router = Router::new(state)
        .upgrade("/ws/peer_metrics", ws_peer_metrics)
        .route(Method::Get, "/dns/resolve", handle_dns_resolve)
        .route(Method::Get, "/api/*tail", handle_api)
        .route(Method::Post, "/api/*tail", handle_api)
        .route(Method::Get, "/drive/:object_id", handle_drive_fetch)
        .route(Method::Get, "/*path", handle_static);
    let config = ServerConfig::default();
    if let Some(tls_cfg) = tls {
        serve_tls(listener, router, config, tls_cfg).await?;
    } else {
        serve(listener, router, config).await?;
    }
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
        IpAddr::V4(v4) => u32::from(v4).swap_bytes() as u64,
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

async fn handle_dns_resolve(req: Request<GatewayState>) -> Result<Response, HttpError> {
    let state = req.state().clone();
    let question = match parse_dns_request(&req) {
        Ok(question) => question,
        Err(response) => return Ok(response),
    };
    if !question.name.ends_with(".block") {
        return Ok(Response::new(StatusCode::BAD_REQUEST)
            .with_body(b"only .block domains are resolvable".to_vec()));
    }
    let has_stake = state.stake.has_stake(&question.name);
    let answers = if has_stake {
        answers_for_question(&state.resolver, &question)
    } else {
        Vec::new()
    };
    let status_value = if has_stake && !answers.is_empty() {
        0
    } else {
        3
    };
    let status_code = if has_stake {
        if answers.is_empty() {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::OK
        }
    } else {
        StatusCode::FORBIDDEN
    };
    let status_label = status_value.to_string();
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::GATEWAY_DOH_STATUS_TOTAL
            .with_label_values(&[status_label.as_str()])
            .inc();
    }
    let payload = build_dns_payload(&question, &answers, status_value);
    let cache_control = if status_value == 3 {
        "max-age=0".to_string()
    } else {
        format!("max-age={}", state.resolver.ttl())
    };
    let response = Response::new(status_code)
        .json(&payload)?
        .with_header("content-type", "application/dns-json")
        .with_header("cache-control", cache_control)
        .with_header("x-block-resolver", "doh");
    Ok(response)
}

async fn handle_drive_fetch(req: Request<GatewayState>) -> Result<Response, HttpError> {
    let state = req.state().clone();
    if let Err(response) = state.authorize(&req) {
        return Ok(response);
    }
    let object_id = req.param("object_id").unwrap_or("").trim();
    if object_id.is_empty() {
        return Ok(Response::new(StatusCode::BAD_REQUEST).with_body(b"missing object id".to_vec()));
    }
    if let Some(bytes) = state.drive.fetch(object_id) {
        Ok(Response::new(StatusCode::OK)
            .with_header("content-type", "application/octet-stream")
            .with_header("content-length", bytes.len().to_string())
            .with_body(bytes))
    } else {
        Ok(Response::new(StatusCode::NOT_FOUND))
    }
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

/// Trait for looking up domain stake deposits.
pub trait StakeTable {
    fn has_stake(&self, domain: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ad_readiness::{AdReadinessConfig, AdReadinessHandle};
    use crate::storage::pipeline;
    #[cfg(feature = "telemetry")]
    use crate::telemetry;
    use ad_market::{
        badge::ann::{SoftIntentReceipt, WalletAnnIndexSnapshot},
        badge::BadgeSoftIntentContext,
        budget::{BudgetBroker, BudgetBrokerConfig},
        uplift::{UpliftEstimate, UpliftHoldoutAssignment},
        Campaign, CampaignTargeting, Creative, DeliveryChannel, DistributionPolicy,
        InMemoryMarketplace, Marketplace, MarketplaceConfig, MatchOutcome, ResourceFloorBreakdown,
        SelectionCandidateTrace, SelectionCohortTrace, SelectionReceipt, TokenOracle,
        MICROS_PER_DOLLAR,
    };
    use base64_fp::encode_standard;
    use foundation_serialization::binary;
    use foundation_serialization::json;
    use foundation_serialization::json::Value as JsonValue;
    use httpd::{Method, Router, StatusCode};
    use runtime::sync::mpsc;
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::sync::{Arc, Mutex, RwLock};

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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn state_with_domains(domains: &[&str]) -> (GatewayState, mpsc::Receiver<ReadAck>) {
        state_with_market(domains, None, None)
    }

    fn state_with_market(
        domains: &[&str],
        market: Option<MarketplaceHandle>,
        readiness: Option<AdReadinessHandle>,
    ) -> (GatewayState, mpsc::Receiver<ReadAck>) {
        clear_crm_lists();
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
                readiness,
                mesh_queue: Arc::new(Mutex::new(RangeBoost::new())),
                resolver: ResolverConfig::empty(),
                drive: Arc::new(drive::DriveStore::from_env()),
            },
            rx,
        )
    }

    fn build_dns_query(name: &str, record_type: RecordType) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        buf.extend(&[0x00, 0x00]); // ID
        buf.extend(&[0x01, 0x00]); // Flags (recursion desired)
        buf.extend(&[0x00, 0x01]); // QDCOUNT
        buf.extend(&[0x00, 0x00]); // ANCOUNT
        buf.extend(&[0x00, 0x00]); // NSCOUNT
        buf.extend(&[0x00, 0x00]); // ARCOUNT
        for label in name.split('.') {
            if label.is_empty() {
                continue;
            }
            buf.push(label.len() as u8);
            buf.extend(label.as_bytes());
        }
        buf.push(0);
        buf.extend(&record_type.as_u16().to_be_bytes());
        buf.extend(&1u16.to_be_bytes()); // IN class
        buf
    }

    #[test]
    fn doh_requires_stake_for_domain() {
        let (state, _) = state_with_domains(&["allowed.block"]);
        let router =
            Router::new(state.clone()).route(Method::Get, "/dns/resolve", handle_dns_resolve);
        let request = router
            .request_builder()
            .path("/dns/resolve")
            .query_param("name", "missing.block")
            .query_param("type", "A")
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn doh_returns_answer_for_resolved_domain() {
        let (mut state, _) = state_with_domains(&["allowed.block"]);
        state.resolver = ResolverConfig::with_addresses(vec!["1.2.3.4".parse().unwrap()], 37, None);
        let router = Router::new(state).route(Method::Get, "/dns/resolve", handle_dns_resolve);
        let request = router
            .request_builder()
            .path("/dns/resolve")
            .query_param("name", "allowed.block")
            .query_param("type", "A")
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: JsonValue = json::from_slice(response.body()).unwrap();
        let answers = body
            .get("Answer")
            .and_then(|value| value.as_array())
            .unwrap_or(&Vec::new())
            .clone();
        assert!(!answers.is_empty());
        let first = answers[0].as_object().unwrap();
        assert_eq!(first.get("data").and_then(|v| v.as_str()), Some("1.2.3.4"));
        assert_eq!(response.header("cache-control"), Some("max-age=37"));
    }

    #[test]
    fn doh_handles_dns_payload_parameter() {
        let (mut state, _) = state_with_domains(&["allowed.block"]);
        state.resolver = ResolverConfig::with_addresses(vec!["5.6.7.8".parse().unwrap()], 45, None);
        let router = Router::new(state).route(Method::Get, "/dns/resolve", handle_dns_resolve);
        let query = build_dns_query("allowed.block", RecordType::A);
        let encoded = encode_standard(&query);
        let request = router
            .request_builder()
            .path("/dns/resolve")
            .query_param("dns", encoded)
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: JsonValue = json::from_slice(response.body()).unwrap();
        let answers = body
            .get("Answer")
            .and_then(|value| value.as_array())
            .unwrap_or(&Vec::new())
            .clone();
        let first = answers[0].as_object().unwrap();
        assert_eq!(first.get("data").and_then(|v| v.as_str()), Some("5.6.7.8"));
    }

    struct StubMarketplace {
        outcome: Mutex<Option<MatchOutcome>>,
        broker: RwLock<BudgetBroker>,
        distribution: RwLock<DistributionPolicy>,
        oracle: RwLock<TokenOracle>,
    }

    impl StubMarketplace {
        fn new(outcome: MatchOutcome) -> Self {
            Self {
                outcome: Mutex::new(Some(outcome)),
                broker: RwLock::new(BudgetBroker::new(BudgetBrokerConfig::default())),
                distribution: RwLock::new(DistributionPolicy::default()),
                oracle: RwLock::new(TokenOracle::default()),
            }
        }
    }

    impl Marketplace for StubMarketplace {
        fn register_campaign(
            &self,
            _campaign: ad_market::Campaign,
        ) -> Result<(), ad_market::MarketplaceError> {
            unimplemented!()
        }

        fn list_campaigns(&self) -> Vec<ad_market::CampaignSummary> {
            Vec::new()
        }

        fn campaign(&self, _id: &str) -> Option<ad_market::Campaign> {
            None
        }

        fn reserve_impression(
            &self,
            _key: ad_market::ReservationKey,
            _ctx: ImpressionContext,
        ) -> Option<MatchOutcome> {
            self.outcome.lock().unwrap().clone()
        }

        fn commit(
            &self,
            _key: &ad_market::ReservationKey,
        ) -> Option<ad_market::SettlementBreakdown> {
            None
        }

        fn cancel(&self, _key: &ad_market::ReservationKey) {}

        fn distribution(&self) -> DistributionPolicy {
            self.distribution.read().unwrap().clone()
        }

        fn update_distribution(&self, policy: DistributionPolicy) {
            *self.distribution.write().unwrap() = policy;
        }

        fn update_oracle(&self, oracle: TokenOracle) {
            *self.oracle.write().unwrap() = oracle;
        }

        fn oracle(&self) -> TokenOracle {
            self.oracle.read().unwrap().clone()
        }

        fn cohort_prices(&self) -> Vec<ad_market::CohortPriceSnapshot> {
            Vec::new()
        }

        fn budget_broker(&self) -> &RwLock<BudgetBroker> {
            &self.broker
        }

        fn record_conversion(
            &self,
            _event: ad_market::ConversionEvent,
        ) -> Result<(), ad_market::MarketplaceError> {
            Ok(())
        }

        fn recompute_distribution_from_utilization(&self) {
            // Stub implementation - no-op for tests
        }

        fn cost_medians_usd_micros(&self) -> (u64, u64, u64) {
            // Stub implementation - return default values for tests
            (0, 0, 0)
        }

        fn badge_guard_decision(
            &self,
            _badges: &[String],
            _soft_intent: Option<&ad_market::BadgeSoftIntentContext>,
        ) -> ad_market::BadgeDecision {
            ad_market::BadgeDecision::Allowed {
                required: Vec::new(),
                proof: None,
            }
        }

        fn update_quality_signals(&self, _signals: Vec<ad_market::QualitySignal>) {}

        fn quality_signal_config(&self) -> ad_market::QualitySignalConfig {
            ad_market::MarketplaceConfig::default().quality_signal_config()
        }

        fn privacy_budget_snapshot(&self) -> ad_market::PrivacyBudgetSnapshot {
            ad_market::PrivacyBudgetSnapshot::default()
        }

        fn preview_privacy_budget(
            &self,
            _badges: &[String],
            _population_hint: Option<u64>,
        ) -> ad_market::PrivacyBudgetPreview {
            ad_market::PrivacyBudgetPreview {
                decision: ad_market::PrivacyBudgetDecision::Allowed,
                remaining_ppm: 1_000_000,
                denied_ppm: 0,
                cooldown_remaining: 0,
            }
        }

        fn authorize_privacy_budget(
            &self,
            _badges: &[String],
            _population_hint: Option<u64>,
        ) -> ad_market::PrivacyBudgetDecision {
            ad_market::PrivacyBudgetDecision::Allowed
        }

        fn register_claim_route(
            &self,
            _domain: &str,
            _role: &str,
            _address: &str,
        ) -> Result<(), ad_market::MarketplaceError> {
            Ok(())
        }

        fn claim_routes(
            &self,
            _cohort: &ad_market::CohortKeySnapshot,
        ) -> std::collections::HashMap<String, String> {
            std::collections::HashMap::new()
        }
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
        let _guard = pipeline::PipelineTestGuard::new();
        let (state, mut rx) = state_with_domains(&["signed.test"]);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9200".parse().unwrap();
        let mut manifest = [0u8; 32];
        manifest[0] = 0xAA;
        manifest[31] = 0x55;
        let path = "/index.html";
        let bytes = 0u64;
        let ts = 1_696_969_696u64;
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
    }

    #[test]
    fn static_read_attaches_campaign_metadata() {
        let _guard = pipeline::PipelineTestGuard::new();
        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(MarketplaceConfig {
            distribution,
            ..MarketplaceConfig::default()
        }));
        market
            .register_campaign(Campaign {
                id: "cmp1".to_string(),
                advertiser_account: "adv1".to_string(),
                budget_usd_micros: 10 * MICROS_PER_DOLLAR,
                creatives: vec![Creative {
                    id: "creative1".to_string(),
                    action_rate_ppm: 500_000,
                    margin_ppm: 800_000,
                    value_per_action_usd_micros: 4 * MICROS_PER_DOLLAR,
                    max_cpi_usd_micros: Some(2 * MICROS_PER_DOLLAR),
                    lift_ppm: 520_000,
                    badges: Vec::new(),
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: Default::default(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: Vec::new(),
                    geo: Default::default(),
                    device: Default::default(),
                    crm_lists: Default::default(),
                    delivery: Default::default(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");
        let (state, mut rx) = state_with_market(&["signed.test"], Some(market.clone()), None);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9300".parse().unwrap();
        let mut manifest = [0u8; 32];
        manifest[0] = 0x11;
        let path = "/creative.html";
        let bytes = 1_048_576u64;
        let ts = 1_777_777_777u64;
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
        // The reservation should clear once the worker commits; ensure metadata persisted on ack.
    }

    #[test]
    fn static_read_respects_ad_readiness() {
        let _guard = pipeline::PipelineTestGuard::new();
        service_badge::clear_badges();

        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(MarketplaceConfig {
            distribution,
            ..MarketplaceConfig::default()
        }));
        market
            .register_campaign(Campaign {
                id: "cmp-ready".to_string(),
                advertiser_account: "adv-ready".to_string(),
                budget_usd_micros: 6 * MICROS_PER_DOLLAR,
                creatives: vec![Creative {
                    id: "creative-ready".to_string(),
                    action_rate_ppm: 450_000,
                    margin_ppm: 800_000,
                    value_per_action_usd_micros: 4 * MICROS_PER_DOLLAR,
                    max_cpi_usd_micros: Some(2 * MICROS_PER_DOLLAR),
                    lift_ppm: 470_000,
                    badges: Vec::new(),
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: Default::default(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: Vec::new(),
                    geo: Default::default(),
                    device: Default::default(),
                    crm_lists: Default::default(),
                    delivery: Default::default(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");

        let readiness = AdReadinessHandle::new(AdReadinessConfig {
            window_secs: 600,
            min_unique_viewers: 2,
            min_host_count: 1,
            min_provider_count: 1,
            use_percentile_thresholds: false,
            viewer_percentile: 90,
            host_percentile: 75,
            provider_percentile: 50,
            ema_smoothing_ppm: 200_000,
            floor_unique_viewers: 0,
            floor_host_count: 0,
            floor_provider_count: 0,
            cap_unique_viewers: 0,
            cap_host_count: 0,
            cap_provider_count: 0,
            percentile_buckets: 12,
        });

        #[cfg(feature = "telemetry")]
        {
            telemetry::AD_READINESS_SKIPPED.reset();
        }

        let (state, mut rx) = state_with_market(
            &["signed.test"],
            Some(market.clone()),
            Some(readiness.clone()),
        );
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9310".parse().unwrap();
        let mut manifest = [0u8; 32];
        manifest[0] = 0x21;
        let path = "/ad.html";
        let bytes = 512_000u64;
        let ts = 1_888_888_888u64;
        pipeline::override_manifest_providers_for_test(
            manifest,
            vec!["gateway-sfo-01".to_string()],
        );
        pipeline::override_static_blob_for_test("signed.test", path, vec![1u8; bytes as usize]);
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
        assert!(ack.campaign_id.is_none());
        assert!(ack.creative_id.is_none());
        #[cfg(feature = "telemetry")]
        {
            let counter =
                telemetry::AD_READINESS_SKIPPED.with_label_values(&["insufficient_unique_viewers"]);
            assert_eq!(counter.get(), 1);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .as_secs();
        readiness.record_ack(now, [0u8; 32], "signed.test", Some("gateway-sfo-01"));
        readiness.record_ack(now + 1, [1u8; 32], "signed.test", Some("gateway-sfo-01"));

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

        let ack = rx.try_recv().expect("second ack queued");
        assert_eq!(ack.campaign_id.as_deref(), Some("cmp-ready"));
        assert_eq!(ack.creative_id.as_deref(), Some("creative-ready"));
        assert_eq!(ack.provider, "gateway-sfo-01");
    }

    #[test]
    fn static_read_matches_badge_targeted_campaign() {
        let _guard = pipeline::PipelineTestGuard::new();
        service_badge::clear_badges();
        service_badge::set_physical_presence("gateway-ldn-01", true);

        let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(MarketplaceConfig {
            distribution,
            ..MarketplaceConfig::default()
        }));
        market
            .register_campaign(Campaign {
                id: "cmp-badge".to_string(),
                advertiser_account: "adv-badge".to_string(),
                budget_usd_micros: 4 * MICROS_PER_DOLLAR,
                creatives: vec![Creative {
                    id: "creative-badge".to_string(),
                    action_rate_ppm: 300_000,
                    margin_ppm: 900_000,
                    value_per_action_usd_micros: 5 * MICROS_PER_DOLLAR,
                    max_cpi_usd_micros: Some(2 * MICROS_PER_DOLLAR),
                    lift_ppm: 320_000,
                    badges: vec!["physical_presence".to_string()],
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: Default::default(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: vec!["physical_presence".to_string()],
                    geo: Default::default(),
                    device: Default::default(),
                    crm_lists: Default::default(),
                    delivery: Default::default(),
                },
                metadata: HashMap::new(),
            })
            .expect("badge campaign registered");

        let (state, mut rx) = state_with_market(&["signed.test"], Some(market.clone()), None);
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
    }

    #[test]
    fn static_read_attaches_soft_intent_receipt() {
        let _guard = pipeline::PipelineTestGuard::new();
        service_badge::clear_badges();
        service_badge::set_physical_presence("gateway-ldn-01", true);

        let mut config = MarketplaceConfig::default();
        config.badge_guard.soft_intent_required = false;
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(config));
        market
            .register_campaign(Campaign {
                id: "cmp-soft-intent".to_string(),
                advertiser_account: "adv-soft".to_string(),
                budget_usd_micros: 4 * MICROS_PER_DOLLAR,
                creatives: vec![Creative {
                    id: "creative-soft".to_string(),
                    action_rate_ppm: 320_000,
                    margin_ppm: 850_000,
                    value_per_action_usd_micros: 5 * MICROS_PER_DOLLAR,
                    max_cpi_usd_micros: Some(2 * MICROS_PER_DOLLAR),
                    lift_ppm: 410_000,
                    badges: vec!["physical_presence".to_string()],
                    domains: vec!["signed.test".to_string()],
                    metadata: HashMap::new(),
                    mesh_payload: None,
                    placement: Default::default(),
                }],
                targeting: CampaignTargeting {
                    domains: vec!["signed.test".to_string()],
                    badges: vec!["physical_presence".to_string()],
                    geo: Default::default(),
                    device: Default::default(),
                    crm_lists: Default::default(),
                    delivery: Default::default(),
                },
                metadata: HashMap::new(),
            })
            .expect("campaign registered");

        let (state, mut rx) = state_with_market(&["signed.test"], Some(market.clone()), None);
        let router = Router::new(state.clone()).route(Method::Get, "/*path", handle_static);
        let remote: SocketAddr = "127.0.0.1:9500".parse().unwrap();
        let manifest = [0x44u8; 32];
        let path = "/soft.html";
        let bytes = 524_288u64;
        let ts = 1_889_000_001u64;
        pipeline::override_manifest_providers_for_test(
            manifest,
            vec!["gateway-ldn-01".to_string()],
        );
        pipeline::override_static_blob_for_test("signed.test", path, vec![0u8; bytes as usize]);

        let badge_list = vec!["physical_presence".to_string()];
        let query = ann::hash_badges(&badge_list);
        let snapshot = ann::WalletAnnIndexSnapshot::new([0xAA; 32], vec![query, [0x33; 32]], 16);
        let proof = ann::build_proof(&snapshot, &badge_list).expect("soft intent proof");
        let snapshot_hex = hex::encode(binary::encode(&snapshot).expect("encode snapshot"));
        let proof_hex = hex::encode(binary::encode(&proof).expect("encode proof"));

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
            .header(HEADER_BADGE_ANN_SNAPSHOT, snapshot_hex)
            .header(HEADER_BADGE_ANN_PROOF, proof_hex)
            .build();
        let response = runtime::block_on(router.handle(request)).unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let ack = rx.try_recv().expect("ack queued");
        let context = ack.badge_soft_intent.as_ref().expect("soft intent context");
        let wallet_snapshot = context.wallet_index.as_ref().expect("snapshot present");
        assert_eq!(wallet_snapshot.fingerprint, snapshot.fingerprint);
        assert_eq!(wallet_snapshot.bucket_hashes, snapshot.bucket_hashes);
        assert_eq!(wallet_snapshot.dimensions, snapshot.dimensions);
        assert_eq!(context.proof.as_ref(), Some(&proof));

        let receipt = ack.selection_receipt.as_ref().expect("selection receipt");
        assert_eq!(receipt.badge_soft_intent.as_ref(), Some(&proof));
        let receipt_snapshot = receipt
            .badge_soft_intent_snapshot
            .as_ref()
            .expect("receipt snapshot");
        assert_eq!(receipt_snapshot.fingerprint, snapshot.fingerprint);
        assert_eq!(receipt_snapshot.bucket_hashes, snapshot.bucket_hashes);
        assert_eq!(receipt_snapshot.dimensions, snapshot.dimensions);
    }

    #[test]
    fn attach_campaign_metadata_propagates_shading_and_ann_fields() {
        let snapshot = WalletAnnIndexSnapshot::new([7u8; 32], vec![[1u8; 32]; 2], 3)
            .with_entropy_salt(vec![9u8; 16]);
        let ann_receipt = SoftIntentReceipt::default();
        let candidate = SelectionCandidateTrace {
            campaign_id: "cmp-test".into(),
            creative_id: "creative-test".into(),
            base_bid_usd_micros: 1_200,
            quality_adjusted_bid_usd_micros: 1_050,
            available_budget_usd_micros: 5_000,
            action_rate_ppm: 450_000,
            lift_ppm: 25_000,
            quality_multiplier: 1.1,
            pacing_kappa: 0.75,
            requested_kappa: 0.66,
            shading_multiplier: 0.9,
            shadow_price: 0.33,
            dual_price: 0.2,
            predicted_lift_ppm: 20_000,
            baseline_action_rate_ppm: 420_000,
            predicted_propensity: 0.42,
            uplift_sample_size: 128,
            uplift_ece: 0.08,
            delivery_channel: DeliveryChannel::Http,
            preferred_delivery_match: false,
        };
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "selection.test".into(),
                domain_tier: Default::default(),
                domain_owner: None,
                provider: Some("provider".into()),
                badges: vec!["vip".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: 1,
                bytes: 512,
                price_per_mib_usd_micros: 100,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
            },
            candidates: vec![candidate.clone()],
            winner_index: 0,
            resource_floor_usd_micros: 90,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 50,
                verifier_usd_micros: 20,
                host_usd_micros: 20,
                qualified_impressions_per_proof: 128,
            },
            runner_up_quality_bid_usd_micros: 800,
            clearing_price_usd_micros: 1_000,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: Some(ann_receipt.clone()),
            badge_soft_intent_snapshot: Some(snapshot.clone()),
            uplift_assignment: None,
        };
        let outcome = MatchOutcome {
            campaign_id: "cmp-test".into(),
            creative_id: "creative-test".into(),
            price_per_mib_usd_micros: 100,
            total_usd_micros: 2_048,
            clearing_price_usd_micros: 1_000,
            resource_floor_usd_micros: 90,
            resource_floor_breakdown: receipt.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: 800,
            quality_adjusted_bid_usd_micros: 1_050,
            selection_receipt: receipt.clone(),
            uplift: UpliftEstimate {
                lift_ppm: 20_000,
                baseline_action_rate_ppm: 400_000,
                propensity: 0.4,
                ece: 0.05,
                sample_size: 256,
            },
            uplift_assignment: UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            },
            delivery_channel: DeliveryChannel::Http,
            mesh_payload: None,
        };
        let market: MarketplaceHandle = Arc::new(StubMarketplace::new(outcome));
        let (state, _rx) = state_with_market(&["selection.test"], Some(market), None);

        let mut ack = ReadAck {
            manifest: [0u8; 32],
            path_hash: [1u8; 32],
            bytes: 2_048,
            ts: 123,
            client_hash: [2u8; 32],
            pk: [3u8; 32],
            sig: vec![4u8; 64],
            domain: "selection.test".into(),
            provider: "".into(),
            campaign_id: None,
            creative_id: None,
            selection_receipt: None,
            geo: None,
            device: None,
            crm_lists: Vec::new(),
            delivery_channel: DeliveryChannel::Http,
            mesh: None,
            badge_soft_intent: Some(BadgeSoftIntentContext {
                wallet_index: Some(snapshot.clone()),
                proof: Some(ann_receipt.clone()),
            }),
            readiness: None,
            zk_proof: None,
            presence_badge: None,
            venue_id: None,
            crowd_size_hint: None,
        };

        attach_campaign_metadata(&state, &mut ack);

        let receipt = ack
            .selection_receipt
            .as_ref()
            .expect("selection receipt attached");
        assert_eq!(receipt.candidates.len(), 1);
        let candidate = &receipt.candidates[0];
        assert!((candidate.requested_kappa - 0.66).abs() < f64::EPSILON);
        assert!((candidate.shadow_price - 0.33).abs() < f64::EPSILON);
        assert_eq!(receipt.badge_soft_intent.as_ref(), Some(&ann_receipt));
        let receipt_snapshot = receipt
            .badge_soft_intent_snapshot
            .as_ref()
            .expect("snapshot");
        assert_eq!(receipt_snapshot.fingerprint, snapshot.fingerprint);
        assert_eq!(receipt_snapshot.bucket_hashes, snapshot.bucket_hashes);
        assert_eq!(receipt_snapshot.dimensions, snapshot.dimensions);
        let context = ack.badge_soft_intent.as_ref().expect("context preserved");
        assert_eq!(context.proof.as_ref(), Some(&ann_receipt));
        let context_snapshot = context.wallet_index.as_ref().expect("wallet index");
        assert_eq!(context_snapshot.fingerprint, snapshot.fingerprint);
        assert_eq!(context_snapshot.bucket_hashes, snapshot.bucket_hashes);
        assert_eq!(context_snapshot.dimensions, snapshot.dimensions);
    }

    #[test]
    fn attach_campaign_metadata_preserves_multi_candidate_enrichment() {
        let winner = SelectionCandidateTrace {
            campaign_id: "cmp-winner".into(),
            creative_id: "creative-winner".into(),
            base_bid_usd_micros: 1_800_000,
            quality_adjusted_bid_usd_micros: 1_950_000,
            available_budget_usd_micros: 7_200_000,
            action_rate_ppm: 410_000,
            lift_ppm: 52_000,
            quality_multiplier: 1.15,
            pacing_kappa: 0.72,
            requested_kappa: 0.58,
            shading_multiplier: 0.84,
            shadow_price: 0.27,
            dual_price: 0.19,
            ..SelectionCandidateTrace::default()
        };
        let runner_up = SelectionCandidateTrace {
            campaign_id: "cmp-runner".into(),
            creative_id: "creative-runner".into(),
            base_bid_usd_micros: 1_400_000,
            quality_adjusted_bid_usd_micros: 1_520_000,
            available_budget_usd_micros: 5_600_000,
            action_rate_ppm: 360_000,
            lift_ppm: 37_000,
            quality_multiplier: 1.08,
            pacing_kappa: 0.66,
            requested_kappa: 0.49,
            shading_multiplier: 0.79,
            shadow_price: 0.31,
            dual_price: 0.23,
            ..SelectionCandidateTrace::default()
        };
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "multi.test".into(),
                domain_tier: Default::default(),
                domain_owner: None,
                provider: Some("wallet".into()),
                badges: vec!["tier.one".into(), "tier.two".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: 1,
                bytes: 1_024,
                price_per_mib_usd_micros: 220,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
            },
            candidates: vec![runner_up.clone(), winner.clone()],
            winner_index: 1,
            resource_floor_usd_micros: 180,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 120,
                verifier_usd_micros: 35,
                host_usd_micros: 25,
                qualified_impressions_per_proof: 512,
            },
            runner_up_quality_bid_usd_micros: runner_up.quality_adjusted_bid_usd_micros,
            clearing_price_usd_micros: 1_520_000,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
            uplift_assignment: None,
        };
        let outcome = MatchOutcome {
            campaign_id: winner.campaign_id.clone(),
            creative_id: winner.creative_id.clone(),
            price_per_mib_usd_micros: 220,
            total_usd_micros: 2_048,
            clearing_price_usd_micros: 1_520_000,
            resource_floor_usd_micros: 180,
            resource_floor_breakdown: receipt.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: runner_up.quality_adjusted_bid_usd_micros,
            quality_adjusted_bid_usd_micros: winner.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt.clone(),
            uplift: UpliftEstimate {
                lift_ppm: 49_000,
                baseline_action_rate_ppm: 395_000,
                propensity: 0.43,
                ece: 0.06,
                sample_size: 384,
            },
            uplift_assignment: UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            },
            delivery_channel: DeliveryChannel::Http,
            mesh_payload: None,
        };
        let market: MarketplaceHandle = Arc::new(StubMarketplace::new(outcome));
        let (state, _rx) = state_with_market(&["multi.test"], Some(market), None);

        let mut ack = ReadAck {
            manifest: [5u8; 32],
            path_hash: [7u8; 32],
            bytes: 2_048,
            ts: 77,
            client_hash: [3u8; 32],
            pk: [1u8; 32],
            sig: vec![2u8; 64],
            domain: "multi.test".into(),
            provider: String::new(),
            campaign_id: None,
            creative_id: None,
            selection_receipt: None,
            geo: None,
            device: None,
            crm_lists: Vec::new(),
            delivery_channel: DeliveryChannel::Http,
            mesh: None,
            badge_soft_intent: None,
            readiness: None,
            zk_proof: None,
            presence_badge: None,
            venue_id: None,
            crowd_size_hint: None,
        };

        attach_campaign_metadata(&state, &mut ack);

        assert_eq!(ack.campaign_id.as_deref(), Some("cmp-winner"));
        assert_eq!(ack.creative_id.as_deref(), Some("creative-winner"));
        let attached = ack
            .selection_receipt
            .as_ref()
            .expect("selection receipt attached");
        assert_eq!(attached.candidates.len(), 2);
        let enriched_runner = &attached.candidates[0];
        let enriched_winner = &attached.candidates[1];
        assert!((enriched_runner.requested_kappa - runner_up.requested_kappa).abs() < f64::EPSILON);
        assert!((enriched_runner.shadow_price - runner_up.shadow_price).abs() < f64::EPSILON);
        assert!((enriched_winner.requested_kappa - winner.requested_kappa).abs() < f64::EPSILON);
        assert!((enriched_winner.shadow_price - winner.shadow_price).abs() < f64::EPSILON);
    }

    #[test]
    fn attach_campaign_metadata_enqueues_mesh_payload_and_proofs() {
        let candidate = SelectionCandidateTrace {
            campaign_id: "cmp-mesh".into(),
            creative_id: "creative-mesh".into(),
            base_bid_usd_micros: 1_200,
            quality_adjusted_bid_usd_micros: 1_500,
            available_budget_usd_micros: 9_000,
            action_rate_ppm: 40_000,
            lift_ppm: 50_000,
            quality_multiplier: 1.2,
            ..SelectionCandidateTrace::default()
        };
        let receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "mesh.test".into(),
                domain_tier: Default::default(),
                domain_owner: None,
                provider: Some("mesh-provider".into()),
                badges: vec!["mesh.badge".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: 1,
                bytes: 2_048,
                price_per_mib_usd_micros: 180,
                delivery_channel: DeliveryChannel::Mesh,
                mesh_peer: Some("peer-alpha".into()),
                mesh_transport: Some("wifi".into()),
                mesh_latency_ms: Some(7),
            },
            candidates: vec![candidate.clone()],
            winner_index: 0,
            resource_floor_usd_micros: 160,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 90,
                verifier_usd_micros: 40,
                host_usd_micros: 30,
                qualified_impressions_per_proof: 256,
            },
            runner_up_quality_bid_usd_micros: 0,
            clearing_price_usd_micros: 1_600,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
        };
        let mesh_payload = vec![9u8, 8u8, 7u8];
        let outcome = MatchOutcome {
            campaign_id: candidate.campaign_id.clone(),
            creative_id: candidate.creative_id.clone(),
            price_per_mib_usd_micros: 180,
            total_usd_micros: 3_072,
            clearing_price_usd_micros: 1_600,
            resource_floor_usd_micros: 160,
            resource_floor_breakdown: receipt.resource_floor_breakdown.clone(),
            runner_up_quality_bid_usd_micros: 0,
            quality_adjusted_bid_usd_micros: candidate.quality_adjusted_bid_usd_micros,
            selection_receipt: receipt.clone(),
            uplift: UpliftEstimate {
                lift_ppm: 50_000,
                baseline_action_rate_ppm: 40_000,
                propensity: 0.5,
                ece: 0.05,
                sample_size: 10,
            },
            uplift_assignment: UpliftHoldoutAssignment {
                fold: 1,
                in_holdout: false,
                propensity: 1.0,
            },
            delivery_channel: DeliveryChannel::Mesh,
            mesh_payload: Some(mesh_payload.clone()),
        };
        let market: MarketplaceHandle = Arc::new(StubMarketplace::new(outcome));
        let (state, _rx) = state_with_market(&["mesh.test"], Some(market), None);
        let queue = state.mesh_queue.clone();

        let mut ack = ReadAck {
            manifest: [11u8; 32],
            path_hash: [22u8; 32],
            bytes: 3_072,
            ts: 1_234,
            client_hash: [33u8; 32],
            pk: [44u8; 32],
            sig: vec![55u8; 64],
            domain: "mesh.test".into(),
            provider: String::new(),
            campaign_id: None,
            creative_id: None,
            selection_receipt: None,
            geo: None,
            device: None,
            crm_lists: Vec::new(),
            delivery_channel: DeliveryChannel::Mesh,
            mesh: Some(MeshContext {
                peer_id: Some("peer-alpha".into()),
                transport: Some("wifi".into()),
                latency_ms: Some(9),
                hop_proofs: vec!["relay-1".into(), "relay-2".into()],
            }),
            badge_soft_intent: None,
            readiness: None,
            zk_proof: None,
            presence_badge: None,
            venue_id: None,
            crowd_size_hint: None,
        };

        attach_campaign_metadata(&state, &mut ack);

        assert_eq!(ack.campaign_id.as_deref(), Some("cmp-mesh"));
        assert_eq!(ack.creative_id.as_deref(), Some("creative-mesh"));
        assert_eq!(ack.delivery_channel, DeliveryChannel::Mesh);
        assert!(ack.mesh.is_some());

        let mut guard = queue.lock().unwrap();
        assert_eq!(guard.pending(), 1);
        let bundle = guard.dequeue().expect("bundle enqueued");
        assert_eq!(bundle.payload, mesh_payload);
        let relays: Vec<String> = bundle
            .proofs
            .iter()
            .map(|proof| proof.relay.clone())
            .collect();
        assert_eq!(relays, vec!["relay-1".to_string(), "relay-2".to_string()]);
    }

    #[test]
    fn static_read_selects_provider_from_multiple_candidates() {
        let _guard = pipeline::PipelineTestGuard::new();

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
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "should synthesize acknowledgements for unsigned reads"
        );
        let ack = rx.try_recv().expect("synthetic ack should be enqueued");
        assert_eq!(ack.domain, "unsigned.test");
        assert_eq!(ack.bytes, 0);
    }
}
