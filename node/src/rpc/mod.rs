use crate::{
    compute_market::settlement::{SettleMode, Settlement},
    config::{ReadAckPrivacyMode, RpcConfig},
    consensus::pow::{self, BlockHeader},
    gateway,
    governance::{Params, NODE_GOV_STORE},
    identity::{handle_registry::HandleRegistry, DidRegistry},
    kyc, launch_governor,
    localnet::{validate_proximity, AssistReceipt},
    net,
    net::peer::{DropReason, HandshakeError, PeerMetrics, PeerReputation},
    range_boost,
    simple_db::{names, SimpleDb},
    storage::fs::RentEscrow,
    transaction::FeeLane,
    Blockchain, SignedTransaction, EPOCH_BLOCKS,
};
use ::ad_market::MarketplaceHandle;
use ::storage::{
    contract::StorageContract,
    merkle_proof::{MerkleProof, MerkleTree},
    offer::StorageOffer,
};
use base64_fp::decode_standard;
use concurrency::Lazy;
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use crypto_suite::ConstantTimeEq;
use foundation_rpc::{
    Params as RpcParams, Request as RpcRequest, Response as RpcResponse, RpcError,
};
use foundation_serialization::de::DeserializeOwned;
use foundation_serialization::json::{Map, Number, Value};
#[cfg(feature = "telemetry")]
use foundation_serialization::Deserialize;
use foundation_serialization::{binary, json, Serialize};
use httpd::{
    form_urlencoded, serve, HttpError, Method, Request, Response, Router, ServerConfig, StatusCode,
    WebSocketRequest, WebSocketResponse,
};
use runtime::sync::{
    oneshot,
    semaphore::{OwnedSemaphorePermit, Semaphore},
};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

pub mod ledger;
pub mod limiter;
pub mod scheduler;
use limiter::{ClientState, RpcClientErrorCode};

pub mod ad_market;
#[cfg(feature = "telemetry")]
pub mod analytics;
pub mod bridge;
pub mod client;
pub mod compute_market;
pub mod consensus;
pub mod dex;
pub mod energy;
pub mod governance;
pub mod governor;
pub mod htlc;
pub mod identity;
pub mod inflation;
pub mod jurisdiction;
pub mod light;
pub mod logs;
pub mod peer;
pub mod pos;
pub mod state_stream;
pub mod storage;
pub mod treasury;
pub mod vm;
pub mod vm_trace;

static GOV_PARAMS: Lazy<Mutex<Params>> = Lazy::new(|| Mutex::new(Params::default()));
static LOCALNET_RECEIPTS: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_LOCALNET_DB_PATH").unwrap_or_else(|_| "localnet_db".into());
    Mutex::new(SimpleDb::open_named(names::LOCALNET_RECEIPTS, &path))
});

fn json_map(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn strip_null_last_claim_height(value: &mut Value) {
    if let Value::Object(map) = value {
        if let Some(Value::Array(relayers)) = map.get_mut("relayers") {
            for relayer in relayers {
                if let Value::Object(rel_map) = relayer {
                    if matches!(rel_map.get("last_claim_height"), Some(Value::Null)) {
                        rel_map.remove("last_claim_height");
                    }
                }
            }
        }
    }
}

fn deterministic_chunks_for_object(object_id: &str, chunk_count: usize) -> Vec<Vec<u8>> {
    let count = chunk_count.max(1);
    (0..count)
        .map(|idx| {
            let mut hasher = Hasher::new();
            hasher.update(object_id.as_bytes());
            hasher.update(&idx.to_le_bytes());
            hasher.finalize().as_bytes().to_vec()
        })
        .collect()
}

fn merkle_tree_from_chunks(chunks: &[Vec<u8>]) -> MerkleTree {
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    MerkleTree::build(&chunk_refs).expect("failed to build merkle tree for demo chunks")
}

fn drop_counts_to_value(counts: &HashMap<DropReason, u64>) -> Value {
    let mut entries: Vec<(String, u64)> = counts
        .iter()
        .map(|(reason, count)| (reason.as_ref().to_owned(), *count))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut map = Map::new();
    for (reason, count) in entries {
        map.insert(reason, Value::Number(Number::from(count)));
    }
    Value::Object(map)
}

fn handshake_fail_counts_to_value(counts: &HashMap<HandshakeError, u64>) -> Value {
    let mut entries: Vec<(String, u64)> = counts
        .iter()
        .map(|(error, count)| (error.as_str().to_owned(), *count))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut map = Map::new();
    for (error, count) in entries {
        map.insert(error, Value::Number(Number::from(count)));
    }
    Value::Object(map)
}

fn peer_reputation_value(reputation: &PeerReputation) -> Value {
    let mut map = Map::new();
    map.insert("score".to_string(), Value::from(reputation.score));
    Value::Object(map)
}

fn peer_metrics_to_value(metrics: &PeerMetrics) -> Value {
    let mut map = Map::new();
    map.insert(
        "requests".to_string(),
        Value::Number(Number::from(metrics.requests)),
    );
    map.insert(
        "bytes_sent".to_string(),
        Value::Number(Number::from(metrics.bytes_sent)),
    );
    map.insert(
        "sends".to_string(),
        Value::Number(Number::from(metrics.sends)),
    );
    map.insert("drops".to_string(), drop_counts_to_value(&metrics.drops));
    map.insert(
        "handshake_fail".to_string(),
        handshake_fail_counts_to_value(&metrics.handshake_fail),
    );
    map.insert(
        "handshake_success".to_string(),
        Value::Number(Number::from(metrics.handshake_success)),
    );
    map.insert(
        "last_handshake_ms".to_string(),
        Value::Number(Number::from(metrics.last_handshake_ms)),
    );
    map.insert(
        "tls_errors".to_string(),
        Value::Number(Number::from(metrics.tls_errors)),
    );
    map.insert(
        "reputation".to_string(),
        peer_reputation_value(&metrics.reputation),
    );
    map.insert(
        "last_updated".to_string(),
        Value::Number(Number::from(metrics.last_updated)),
    );
    map.insert("req_avg".to_string(), Value::from(metrics.req_avg));
    map.insert("byte_avg".to_string(), Value::from(metrics.byte_avg));
    map.insert(
        "throttled_until".to_string(),
        Value::Number(Number::from(metrics.throttled_until)),
    );
    map.insert(
        "throttle_reason".to_string(),
        metrics
            .throttle_reason
            .as_ref()
            .map(|reason| Value::String(reason.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "backoff_level".to_string(),
        Value::Number(Number::from(metrics.backoff_level)),
    );
    map.insert(
        "sec_start".to_string(),
        Value::Number(Number::from(metrics.sec_start)),
    );
    Value::Object(map)
}

fn status_value(status: &str) -> Value {
    Value::Object({
        let mut map = Map::new();
        map.insert("status".to_string(), Value::String(status.to_string()));
        map
    })
}

#[cfg(test)]
mod tests {
    use super::{
        drop_counts_to_value, handshake_fail_counts_to_value, peer_metrics_to_value, DropReason,
        HandshakeError,
    };
    use crate::net::peer::{PeerMetrics, PeerReputation};
    use foundation_serialization::json::Value;
    use std::collections::HashMap;

    #[test]
    fn drop_counts_are_deterministically_ordered() {
        let mut counts = HashMap::new();
        counts.insert(DropReason::TooBusy, 3);
        counts.insert(DropReason::Blacklist, 1);
        counts.insert(DropReason::RateLimit, 2);

        let value = drop_counts_to_value(&counts);
        let Value::Object(map) = value else {
            panic!("expected object value");
        };

        let keys: Vec<_> = map.keys().cloned().collect();
        assert_eq!(keys, vec!["blacklist", "rate_limit", "too_busy"]);

        assert_eq!(map["blacklist"].as_u64(), Some(1));
        assert_eq!(map["rate_limit"].as_u64(), Some(2));
        assert_eq!(map["too_busy"].as_u64(), Some(3));
    }

    #[test]
    fn handshake_fail_counts_are_deterministically_ordered() {
        let mut counts = HashMap::new();
        counts.insert(HandshakeError::Timeout, 5);
        counts.insert(HandshakeError::Tls, 7);
        counts.insert(HandshakeError::Certificate, 2);

        let value = handshake_fail_counts_to_value(&counts);
        let Value::Object(map) = value else {
            panic!("expected object value");
        };

        let keys: Vec<_> = map.keys().cloned().collect();
        assert_eq!(keys, vec!["certificate", "timeout", "tls"]);

        assert_eq!(map["certificate"].as_u64(), Some(2));
        assert_eq!(map["timeout"].as_u64(), Some(5));
        assert_eq!(map["tls"].as_u64(), Some(7));
    }

    #[test]
    fn peer_metrics_value_includes_sorted_maps() {
        let mut drops = HashMap::new();
        drops.insert(DropReason::Blacklist, 4);
        drops.insert(DropReason::RateLimit, 9);
        let mut handshake_fail = HashMap::new();
        handshake_fail.insert(HandshakeError::Tls, 3);
        handshake_fail.insert(HandshakeError::Timeout, 1);

        let mut reputation = PeerReputation::default();
        reputation.score = 0.75;

        let metrics = PeerMetrics {
            requests: 11,
            bytes_sent: 22,
            sends: 5,
            drops,
            handshake_fail,
            handshake_success: 6,
            last_handshake_ms: 1234,
            tls_errors: 2,
            reputation,
            last_updated: 9999,
            req_avg: 1.5,
            byte_avg: 2.5,
            throttled_until: 555,
            throttle_reason: Some("cooldown".to_string()),
            backoff_level: 3,
            sec_start: 777,
            ..PeerMetrics::default()
        };

        let value = peer_metrics_to_value(&metrics);
        let Value::Object(map) = value else {
            panic!("expected object value");
        };

        assert_eq!(map["requests"].as_u64(), Some(11));
        assert_eq!(map["bytes_sent"].as_u64(), Some(22));
        assert_eq!(map["handshake_success"].as_u64(), Some(6));
        assert_eq!(map["tls_errors"].as_u64(), Some(2));
        assert_eq!(map["last_updated"].as_u64(), Some(9999));
        assert_eq!(map["throttled_until"].as_u64(), Some(555));
        assert_eq!(map["backoff_level"].as_u64(), Some(3));
        assert_eq!(map["sec_start"].as_u64(), Some(777));

        let Value::Object(drop_map) = &map["drops"] else {
            panic!("expected nested drop map");
        };
        let drop_keys: Vec<_> = drop_map.keys().cloned().collect();
        assert_eq!(drop_keys, vec!["blacklist", "rate_limit"]);

        let Value::Object(handshake_map) = &map["handshake_fail"] else {
            panic!("expected nested handshake map");
        };
        let handshake_keys: Vec<_> = handshake_map.keys().cloned().collect();
        assert_eq!(handshake_keys, vec!["timeout", "tls"]);

        assert_eq!(map["throttle_reason"], Value::String("cooldown".into()));
    }
}

fn error_value(message: impl Into<String>) -> Value {
    Value::Object({
        let mut map = Map::new();
        map.insert("error".to_string(), Value::String(message.into()));
        map
    })
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct BadgeStatusResponse {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_mint: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_burn: Option<u64>,
}

struct RpcState {
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
    market: Option<MarketplaceHandle>,
    ad_readiness: Option<crate::ad_readiness::AdReadinessHandle>,
    governor: Option<Arc<launch_governor::GovernorHandle>>,
    clients: Arc<Mutex<HashMap<IpAddr, ClientState>>>,
    tokens_per_sec: f64,
    ban_secs: u64,
    client_timeout: u64,
    concurrent: Arc<Semaphore>,
}

impl RpcState {
    async fn acquire(&self) -> Result<OwnedSemaphorePermit, HttpError> {
        self.concurrent
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| HttpError::Handler("rpc semaphore closed".into()))
    }

    fn check_rate_limit(&self, remote: SocketAddr) -> Option<Response> {
        match limiter::check_client(
            &remote.ip(),
            &self.clients,
            self.tokens_per_sec,
            self.ban_secs,
            self.client_timeout,
        ) {
            Ok(_) => None,
            Err(code) => {
                telemetry_rpc_error(code);
                let err = rpc_error(code.rpc_code(), code.message());
                let resp = RpcResponse::error(err, None);
                let response = Response::new(StatusCode::TOO_MANY_REQUESTS)
                    .json(&resp)
                    .unwrap_or_else(|_| {
                        Response::new(StatusCode::TOO_MANY_REQUESTS).with_body(b"{}".to_vec())
                    })
                    .close();
                Some(response)
            }
        }
    }

    fn apply_cors(&self, mut response: Response, origin: Option<&str>) -> Response {
        if let Some(origin) = origin {
            if self
                .runtime_cfg
                .cors_allow_origins
                .iter()
                .any(|allowed| allowed == origin)
            {
                response = response.with_header("access-control-allow-origin", origin);
            }
        }
        response
    }

    fn is_host_allowed(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return false;
        };
        let normalized = normalize_host_header(host);
        self.runtime_cfg
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(normalized.as_str()))
    }

    fn runtime(&self) -> Arc<RpcRuntimeConfig> {
        Arc::clone(&self.runtime_cfg)
    }
}

fn normalize_host_header(host: &str) -> String {
    let trimmed = host.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return rest[..end].to_string();
        }
    }
    if let Some((name, _)) = trimmed.split_once(':') {
        name.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn render_request_path(request: &Request<RpcState>) -> String {
    if request.query().is_empty() {
        request.path().to_string()
    } else {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        let mut pairs: Vec<_> = request.query().iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in pairs {
            serializer.append_pair(key, value);
        }
        format!("{}?{}", request.path(), serializer.finish())
    }
}

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
    "ad_market.inventory",
    "ad_market.list_campaigns",
    "ad_market.distribution",
    "ad_market.budget",
    "ad_market.broker_state",
    "ad_market.readiness",
    "ad_market.record_conversion",
    "ad_market.list_presence_cohorts",
    "ad_market.reserve_presence",
    "register_handle",
    "resolve_handle",
    "whoami",
    "identity.anchor",
    "identity.resolve",
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
    "le.upload_evidence",
    "service_badge_issue",
    "service_badge_revoke",
    "dns.publish_record",
    "dns.list_for_sale",
    "dns.place_bid",
    "dns.complete_sale",
    "dns.cancel_sale",
    "dns.register_stake",
    "dns.withdraw_stake",
    "dns.stake_status",
    "dns.auctions",
    "gateway.policy",
    "gateway.reads_since",
    "gateway.venue_status",
    "gov.release_signers",
    #[cfg(feature = "telemetry")]
    "analytics",
    "microshard.roots.last",
    "mempool.stats",
    "mempool.qos_event",
    "net.peer_stats",
    "net.peer_stats_all",
    "net.peer_stats_reset",
    "net.peer_stats_export",
    "net.peer_stats_export_all",
    "net.peer_stats_persist",
    "net.peer_throttle",
    "net.overlay_status",
    "net.backpressure_clear",
    "net.reputation_sync",
    "net.reputation_show",
    "net.gossip_status",
    "net.dns_verify",
    "net.key_rotate",
    "net.handshake_failures",
    "net.quic_stats",
    "net.quic_certs",
    "net.quic_certs_refresh",
    "kyc.verify",
    "pow.get_template",
    "dex_escrow_status",
    "dex_escrow_release",
    "dex_escrow_proof",
    "htlc_status",
    "htlc_refund",
    "energy.register_provider",
    "energy.market_state",
    "energy.receipts",
    "energy.credits",
    "energy.disputes",
    "energy.settle",
    "energy.submit_reading",
    "energy.flag_dispute",
    "energy.resolve_dispute",
    "storage_upload",
    "storage_challenge",
    "storage.repair_history",
    "storage.repair_run",
    "storage.repair_chunk",
    "storage.manifests",
    "pow.submit",
    "inflation.params",
    "compute_market.stats",
    "compute_market.provider_balances",
    "compute_market.audit",
    "compute_market.recent_roots",
    "compute_market.sla_history",
    "compute_market.scheduler_metrics",
    "compute_market.scheduler_stats",
    "compute.reputation_get",
    "compute.job_requirements",
    "compute.job_cancel",
    "compute.provider_hardware",
    "governor.status",
    "governor.decisions",
    "governor.snapshot",
    "stake.role",
    "consensus.difficulty",
    "consensus.pos.register",
    "config.reload",
    "consensus.pos.bond",
    "consensus.pos.unbond",
    "consensus.pos.slash",
    "rent.escrow.balance",
    "mesh.peers",
    "light.latest_header",
    "light.headers",
    "light_client.rebate_status",
    "light_client.rebate_history",
    "service_badge_verify",
    "jurisdiction.status",
    "jurisdiction.policy_diff",
];

const ADMIN_METHODS: &[&str] = &[
    "set_snapshot_interval",
    "ad_market.register_campaign",
    "compute_arm_real",
    "compute_cancel_arm",
    "compute_back_to_dry_run",
    "gateway.mobile_cache_status",
    "gateway.mobile_cache_flush",
    "telemetry.configure",
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
    "le.upload_evidence",
];

const DEBUG_METHODS: &[&str] = &["set_difficulty", "start_mining", "stop_mining"];

fn parse_overlay_peer_param(id: &str) -> Result<[u8; 32], RpcError> {
    if id.is_empty() {
        return Err(rpc_error(-32602, "invalid params"));
    }
    if let Ok(peer) = net::overlay_peer_from_base58(id) {
        let mut out = [0u8; 32];
        out.copy_from_slice(peer.as_bytes());
        return Ok(out);
    }
    if let Ok(bytes) = crypto_suite::hex::decode(id) {
        if let Ok(peer) = net::overlay_peer_from_bytes(&bytes) {
            let mut out = [0u8; 32];
            out.copy_from_slice(peer.as_bytes());
            return Ok(out);
        }
    }
    Err(rpc_error(-32602, "invalid params"))
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
        rpc_error(e.code(), e.message())
    }
}

impl From<crate::compute_market::MarketError> for RpcError {
    fn from(e: crate::compute_market::MarketError) -> Self {
        rpc_error(e.code(), e.message())
    }
}

#[doc(hidden)]

fn telemetry_rpc_error(code: RpcClientErrorCode) {
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::RPC_CLIENT_ERROR_TOTAL
            .ensure_handle_for_label_values(&[code.as_str()])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .inc();
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = code;
}

fn rpc_error(code: i32, message: &'static str) -> RpcError {
    RpcError::new(code, message)
}

fn parse_params<T: DeserializeOwned>(params: &RpcParams) -> Result<T, RpcError> {
    let value = match params.as_value() {
        json::Value::Null => json::Value::Object(json::Map::new()),
        other => other.clone(),
    };
    json::from_value(value).map_err(|_| rpc_error(-32602, "invalid params"))
}

fn serialize_response<T: Serialize>(
    value: T,
) -> Result<foundation_serialization::json::Value, RpcError> {
    json::to_value(value).map_err(|_| rpc_error(-32603, "failed to serialize response"))
}

fn check_nonce(
    scope: impl Into<String>,
    params: &RpcParams,
    nonces: &Arc<Mutex<HashSet<(String, u64)>>>,
) -> Result<(), RpcError> {
    let nonce = params
        .get("nonce")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| rpc_error(-32602, "missing nonce"))?;
    let key = scope.into();
    let mut guard = nonces
        .lock()
        .map_err(|_| rpc_error(-32603, "internal error"))?;
    if !guard.insert((key, nonce)) {
        return Err(rpc_error(-32000, "replayed nonce"));
    }
    Ok(())
}

fn execute_rpc(
    state: &RpcState,
    request: RpcRequest,
    auth: Option<&str>,
    peer_ip: Option<IpAddr>,
) -> RpcResponse {
    let runtime_cfg = state.runtime();
    let id = request.id.clone();
    let method_str = request.method.as_str();
    let authorized = match runtime_cfg.admin_token.as_ref() {
        Some(token) => {
            let expected = format!("Bearer {token}");
            auth.map(|h| {
                let a = h.as_bytes();
                let b = expected.as_bytes();
                a.len() == b.len() && a.ct_eq(b).into()
            })
            .unwrap_or(false)
        }
        None => false,
    };

    let dispatch_result = || {
        dispatch(
            &request,
            Arc::clone(&state.bc),
            Arc::clone(&state.mining),
            Arc::clone(&state.nonces),
            Arc::clone(&state.handles),
            Arc::clone(&state.dids),
            Arc::clone(&runtime_cfg),
            state.market.clone(),
            state.ad_readiness.clone(),
            state.governor.clone(),
            auth,
        )
    };

    let response = if DEBUG_METHODS.contains(&method_str) {
        if !runtime_cfg.enable_debug || !authorized {
            Err(rpc_error(-32601, "method not found"))
        } else {
            dispatch_result()
        }
    } else if ADMIN_METHODS.contains(&method_str) {
        if !authorized {
            Err(rpc_error(-32601, "method not found"))
        } else {
            dispatch_result()
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
                Err(rpc_error(-32601, "method not found"))
            } else {
                if method_str == "net.peer_stats_export" {
                    #[cfg(feature = "telemetry")]
                    {
                        if let Some(path) = request.params.get("path").and_then(|v| v.as_str()) {
                            diagnostics::log::info!(
                                "peer_stats_export operator={peer_ip:?} path={path}"
                            );
                        } else {
                            diagnostics::log::info!("peer_stats_export operator={peer_ip:?}");
                        }
                    }
                } else if method_str == "net.peer_stats_export_all" {
                    #[cfg(feature = "telemetry")]
                    diagnostics::log::info!("peer_stats_export_all operator={peer_ip:?}");
                } else if method_str == "net.peer_throttle" {
                    #[cfg(feature = "telemetry")]
                    {
                        let clear = request
                            .params
                            .get("clear")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        diagnostics::log::info!("peer_throttle operator={peer_ip:?} clear={clear}");
                    }
                } else if method_str == "net.backpressure_clear" {
                    #[cfg(feature = "telemetry")]
                    diagnostics::log::info!("backpressure_clear operator={peer_ip:?}");
                }
                dispatch_result()
            }
        } else {
            dispatch_result()
        }
    } else {
        Err(rpc_error(-32601, "method not found"))
    };

    match response {
        Ok(value) => RpcResponse::success(value, id),
        Err(error) => RpcResponse::error(error, id),
    }
}

async fn handle_rpc_options(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(response);
    }
    let origin = request.header("origin");
    let runtime_cfg = state.runtime();
    let allowed_origin = origin.and_then(|candidate| {
        if runtime_cfg
            .cors_allow_origins
            .iter()
            .any(|allowed| allowed == candidate)
        {
            Some(candidate)
        } else {
            None
        }
    });
    if let Some(origin) = allowed_origin {
        let response = Response::new(StatusCode::NO_CONTENT)
            .with_header("access-control-allow-methods", "POST")
            .with_header(
                "access-control-allow-headers",
                "content-type, authorization",
            );
        Ok(state.apply_cors(response, Some(origin)))
    } else {
        Ok(Response::new(StatusCode::FORBIDDEN).close())
    }
}

async fn handle_rpc_post(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    let remote = request.remote_addr();
    if let Some(mut response) = state.check_rate_limit(remote) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let auth = request.header("authorization");
    let peer_ip = Some(remote.ip());
    let origin = request.header("origin");
    let runtime_cfg = state.runtime();
    let rpc_request = match RpcRequest::from_http_state(&request, runtime_cfg.max_body_bytes) {
        Ok(req) => req,
        Err(err) => {
            let response = RpcResponse::error(err.into_rpc_error(), None);
            let response = Response::new(StatusCode::OK).json(&response)?;
            return Ok(state.apply_cors(response, origin));
        }
    };
    let rpc_response = execute_rpc(&state, rpc_request, auth, peer_ip);
    let response = Response::new(StatusCode::OK).json(&rpc_response)?;
    Ok(state.apply_cors(response, origin))
}

async fn handle_logs_search(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let path = render_request_path(&request);
    let (status, body) = logs::search_response(&path);
    let response = Response::new(status)
        .with_header("content-type", "application/json")
        .with_body(body.into_bytes());
    Ok(state.apply_cors(response, request.header("origin")))
}

async fn handle_logs_tail(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    let path = render_request_path(&request);
    match logs::build_tail_config(&path) {
        Ok(cfg) => Ok(WebSocketResponse::accept(move |stream| {
            let permit = permit;
            async move {
                logs::run_tail(stream, cfg).await;
                drop(permit);
                Ok(())
            }
        })),
        Err(err) => {
            let (status, body) = logs::map_search_error(err);
            let response = Response::new(status)
                .with_header("content-type", "application/json")
                .with_body(body.into_bytes())
                .close();
            Ok(WebSocketResponse::Reject(response))
        }
    }
}

async fn handle_vm_trace(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    if !crate::vm::vm_debug_enabled() {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    let code = request
        .query_param("code")
        .and_then(|hex| crypto_suite::hex::decode(hex).ok())
        .unwrap_or_default();
    #[cfg(feature = "telemetry")]
    crate::telemetry::VM_TRACE_TOTAL.inc();
    Ok(WebSocketResponse::accept(move |stream| {
        let permit = permit;
        let code = code.clone();
        async move {
            vm_trace::run_trace(stream, code).await;
            drop(permit);
            Ok(())
        }
    }))
}

async fn handle_state_stream(
    request: Request<RpcState>,
    _upgrade: WebSocketRequest,
) -> Result<WebSocketResponse, HttpError> {
    let state = Arc::clone(request.state());
    let permit = state.acquire().await?;
    if let Some(response) = state.check_rate_limit(request.remote_addr()) {
        return Ok(WebSocketResponse::Reject(response));
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(WebSocketResponse::Reject(response));
    }
    #[cfg(feature = "telemetry")]
    crate::telemetry::STATE_STREAM_SUBSCRIBERS_TOTAL.inc();
    let bc = Arc::clone(&state.bc);
    Ok(WebSocketResponse::accept(move |stream| {
        let permit = permit;
        async move {
            state_stream::run_stream(stream, bc).await;
            drop(permit);
            Ok(())
        }
    }))
}

async fn handle_badge_status(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let (active, last_mint, last_burn) = {
        let mut chain = state.bc.lock().unwrap_or_else(|e| e.into_inner());
        chain.check_badges();
        chain.badge_status()
    };
    let snapshot = BadgeStatusResponse {
        active,
        last_mint,
        last_burn,
    };
    let snapshot_value = foundation_serialization::json::to_value(snapshot).unwrap_or(Value::Null);
    let body = foundation_serialization::json::to_string_value(&snapshot_value);
    let response = Response::new(StatusCode::OK)
        .with_header("content-type", "application/json")
        .with_body(body.into_bytes());
    Ok(state.apply_cors(response, request.header("origin")))
}

async fn handle_dashboard(request: Request<RpcState>) -> Result<Response, HttpError> {
    let state = Arc::clone(request.state());
    let _permit = state.acquire().await?;
    if let Some(mut response) = state.check_rate_limit(request.remote_addr()) {
        response = state.apply_cors(response, request.header("origin"));
        return Ok(response);
    }
    if !state.is_host_allowed(request.header("host")) {
        let response = Response::new(StatusCode::FORBIDDEN).close();
        return Ok(state.apply_cors(response, request.header("origin")));
    }
    let body = include_str!("../../../dashboard/index.html")
        .as_bytes()
        .to_vec();
    let response = Response::new(StatusCode::OK)
        .with_header("content-type", "text/html; charset=utf-8")
        .with_body(body);
    Ok(state.apply_cors(response, request.header("origin")))
}

fn dispatch(
    req: &RpcRequest,
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
    market: Option<MarketplaceHandle>,
    readiness: Option<crate::ad_readiness::AdReadinessHandle>,
    governor: Option<Arc<launch_governor::GovernorHandle>>,
    auth: Option<&str>,
) -> Result<foundation_serialization::json::Value, RpcError> {
    if BADGE_METHODS.contains(&req.method.as_str()) {
        let badge = req
            .params
            .get("badge")
            .and_then(|v| v.as_str())
            .or(req.badge.as_deref())
            .ok_or(rpc_error(-32602, "badge required"))?;
        if !crate::service_badge::verify(badge) {
            return Err(rpc_error(-32602, "invalid badge"));
        }
    }

    let market_ref = market.as_ref();
    let readiness_handle = readiness.clone();
    let readiness_ref = readiness.as_ref();

    Ok(match req.method.as_str() {
        "ad_market.inventory" => ad_market::inventory(market_ref),
        "ad_market.list_campaigns" => ad_market::list_campaigns(market_ref),
        "ad_market.distribution" => ad_market::distribution(market_ref),
        "ad_market.budget" => ad_market::budget(market_ref),
        "ad_market.broker_state" => ad_market::broker_state(market_ref),
        "ad_market.readiness" => ad_market::readiness(market_ref, readiness_ref),
        "ad_market.policy_snapshot" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| rpc_error(-32602, "epoch required"))?;
            let base = bc.lock().unwrap_or_else(|e| e.into_inner()).path.clone();
            match crate::ad_policy_snapshot::load_snapshot(&base, epoch) {
                Some(v) => v,
                None => json_map(vec![("status", Value::String("not_found".into()))]),
            }
        }
        "ad_market.policy_snapshots" => {
            let base = bc.lock().unwrap_or_else(|e| e.into_inner()).path.clone();
            let (start_epoch, end_epoch) = {
                let start = req
                    .params
                    .get("start_epoch")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let end_default = {
                    let g = bc.lock().unwrap_or_else(|e| e.into_inner());
                    g.block_height / 120
                };
                let end = req
                    .params
                    .get("end_epoch")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(end_default);
                (start, end)
            };
            let list = crate::ad_policy_snapshot::list_snapshots(&base, start_epoch, end_epoch);
            let arr = list;
            let mut map = Map::new();
            map.insert("snapshots".into(), Value::Array(arr));
            Value::Object(map)
        }
        "ad_market.register_campaign" => {
            let params = req.params.as_value().clone();
            ad_market::register_campaign(market_ref, &params)?
        }
        "ad_market.record_conversion" => {
            let params = req.params.as_value().clone();
            ad_market::record_conversion(market_ref, &params, auth)?
        }
        "ad_market.list_presence_cohorts" => {
            let params = req.params.as_value().clone();
            ad_market::list_presence_cohorts(market_ref, &params)?
        }
        "ad_market.reserve_presence" => {
            let params = req.params.as_value().clone();
            ad_market::reserve_presence(market_ref, &params)?
        }
        "governor.status" => governor::status(governor.clone())?,
        "governor.decisions" => {
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10);
            governor::decisions(governor.clone(), limit as usize)?
        }
        "governor.snapshot" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| rpc_error(-32602, "epoch required"))?;
            governor::snapshot(governor.clone(), epoch)?
        }
        "set_difficulty" => {
            let val = req
                .params
                .get("value")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            match bc.lock() {
                Ok(mut guard) => {
                    guard.difficulty = val;
                    status_value("ok")
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "node.get_ack_privacy" => {
            let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
            Value::String(guard.config.read_ack_privacy.to_string())
        }
        "node.set_ack_privacy" => {
            let mode_str = req
                .params
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("enforce");
            let mode = mode_str
                .parse::<ReadAckPrivacyMode>()
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            match bc.lock() {
                Ok(mut guard) => {
                    if guard.config.read_ack_privacy != mode {
                        guard.config.read_ack_privacy = mode;
                        guard.save_config();
                    }
                    status_value("ok")
                }
                Err(_) => error_value("lock poisoned"),
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
                json_map(vec![
                    (
                        "consumer",
                        Value::Number(Number::from(acct.balance.consumer)),
                    ),
                    (
                        "industrial",
                        Value::Number(Number::from(acct.balance.industrial)),
                    ),
                ])
            } else {
                json_map(vec![
                    ("consumer", Value::Number(Number::from(0))),
                    ("industrial", Value::Number(Number::from(0))),
                ])
            }
        }
        "ledger.shard_of" => {
            let addr = req
                .params
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let shard = ledger::shard_of(addr);
            json_map(vec![("shard", Value::Number(Number::from(shard)))])
        }
        "anomaly.label" => {
            let _label = req
                .params
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            #[cfg(feature = "telemetry")]
            crate::telemetry::ANOMALY_LABEL_TOTAL.inc();
            status_value("ok")
        }
        "settlement_status" => {
            let provider = req.params.get("provider").and_then(|v| v.as_str());
            let mode = match Settlement::mode() {
                SettleMode::DryRun => "dryrun",
                SettleMode::Real => "real",
                SettleMode::Armed { .. } => "armed",
            };
            if let Some(p) = provider {
                let balance = Settlement::balance(p);
                json_map(vec![
                    ("mode", Value::String(mode.to_string())),
                    ("balance", Value::Number(Number::from(balance))),
                    ("ct", Value::Number(Number::from(balance))),
                ])
            } else {
                json_map(vec![("mode", Value::String(mode.to_string()))])
            }
        }
        "settlement.audit" => compute_market::settlement_audit(),
        "bridge.relayer_status" => {
            let params = parse_params::<bridge::RelayerStatusRequest>(&req.params)?;
            serialize_response(bridge::relayer_status(params)?)?
        }
        "bridge.bond_relayer" => {
            let params = parse_params::<bridge::BondRelayerRequest>(&req.params)?;
            serialize_response(bridge::bond_relayer(params)?)?
        }
        "bridge.claim_rewards" => {
            let params = parse_params::<bridge::ClaimRewardsRequest>(&req.params)?;
            serialize_response(bridge::claim_rewards(params)?)?
        }
        "bridge.verify_deposit" => {
            let params = parse_params::<bridge::VerifyDepositRequest>(&req.params)?;
            serialize_response(bridge::verify_deposit(params)?)?
        }
        "bridge.request_withdrawal" => {
            let params = parse_params::<bridge::RequestWithdrawalRequest>(&req.params)?;
            serialize_response(bridge::request_withdrawal(params)?)?
        }
        "bridge.challenge_withdrawal" => {
            let params = parse_params::<bridge::ChallengeWithdrawalRequest>(&req.params)?;
            serialize_response(bridge::challenge_withdrawal(params)?)?
        }
        "bridge.finalize_withdrawal" => {
            let params = parse_params::<bridge::FinalizeWithdrawalRequest>(&req.params)?;
            serialize_response(bridge::finalize_withdrawal(params)?)?
        }
        "bridge.submit_settlement" => {
            let params = parse_params::<bridge::SubmitSettlementRequest>(&req.params)?;
            serialize_response(bridge::submit_settlement(params)?)?
        }
        "bridge.pending_withdrawals" => {
            let params = parse_params::<bridge::PendingWithdrawalsRequest>(&req.params)?;
            serialize_response(bridge::pending_withdrawals(params)?)?
        }
        "bridge.active_challenges" => {
            let params = parse_params::<bridge::ActiveChallengesRequest>(&req.params)?;
            serialize_response(bridge::active_challenges(params)?)?
        }
        "bridge.relayer_quorum" => {
            let params = parse_params::<bridge::RelayerQuorumRequest>(&req.params)?;
            serialize_response(bridge::relayer_quorum(params)?)?
        }
        "bridge.reward_claims" => {
            let params = parse_params::<bridge::RewardClaimsRequest>(&req.params)?;
            serialize_response(bridge::reward_claims(params)?)?
        }
        "bridge.reward_accruals" => {
            let params = parse_params::<bridge::RewardAccrualsRequest>(&req.params)?;
            serialize_response(bridge::reward_accruals(params)?)?
        }
        "bridge.settlement_log" => {
            let params = parse_params::<bridge::SettlementLogRequest>(&req.params)?;
            serialize_response(bridge::settlement_log(params)?)?
        }
        "bridge.dispute_audit" => {
            let params = parse_params::<bridge::DisputeAuditRequest>(&req.params)?;
            serialize_response(bridge::dispute_audit(params)?)?
        }
        "bridge.relayer_accounting" => {
            let params = parse_params::<bridge::RelayerAccountingRequest>(&req.params)?;
            serialize_response(bridge::relayer_accounting(params)?)?
        }
        "bridge.duty_log" => {
            let params = parse_params::<bridge::DutyLogRequest>(&req.params)?;
            serialize_response(bridge::duty_log(params)?)?
        }
        "bridge.deposit_history" => {
            let params = parse_params::<bridge::DepositHistoryRequest>(&req.params)?;
            serialize_response(bridge::deposit_history(params)?)?
        }
        "bridge.slash_log" => {
            let params = parse_params::<bridge::SlashLogRequest>(&req.params)?;
            serialize_response(bridge::slash_log(params)?)?
        }
        "bridge.assets" => {
            let params = parse_params::<bridge::AssetsRequest>(&req.params)?;
            serialize_response(bridge::assets(params)?)?
        }
        "bridge.configure_asset" => {
            let params = parse_params::<bridge::ConfigureAssetRequest>(&req.params)?;
            serialize_response(bridge::configure_asset(params)?)?
        }
        "localnet.submit_receipt" => {
            let hex = req
                .params
                .get("receipt")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = match crypto_suite::hex::decode(hex) {
                Ok(b) => b,
                Err(_) => return Err(rpc_error(-32602, "invalid params")),
            };
            let receipt: AssistReceipt = match binary::decode(&bytes) {
                Ok(r) => r,
                Err(_) => return Err(rpc_error(-32602, "invalid params")),
            };
            if !receipt.verify()
                || !validate_proximity(receipt.device, receipt.rssi, receipt.rtt_ms)
            {
                return Err(rpc_error(-32002, "invalid receipt"));
            }
            let hash = receipt.hash();
            let key = format!("localnet_receipts/{}", hash);
            let mut db = LOCALNET_RECEIPTS.lock().unwrap_or_else(|e| e.into_inner());
            if db.get(&key).is_some() {
                status_value("ignored")
            } else {
                db.insert(&key, Vec::new());
                status_value("ok")
            }
        }
        "dns.publish_record" => match gateway::dns::publish_record(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.list_for_sale" => match gateway::dns::list_for_sale(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.place_bid" => match gateway::dns::place_bid(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.complete_sale" => match gateway::dns::complete_sale(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.cancel_sale" => match gateway::dns::cancel_sale(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.register_stake" => match gateway::dns::register_stake(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.withdraw_stake" => match gateway::dns::withdraw_stake(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.stake_status" => match gateway::dns::stake_status(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "dns.auctions" => match gateway::dns::auctions(req.params.as_value()) {
            Ok(v) => v,
            Err(e) => return Err(rpc_error(e.code(), e.message())),
        },
        "gateway.policy" => gateway::dns::gateway_policy(req.params.as_value()),
        "gateway.reads_since" => gateway::dns::reads_since(req.params.as_value()),
        "gateway.venue_status" => {
            let obj = req.params.as_value();
            let venue = obj.get("venue_id").and_then(|v| v.as_str()).unwrap_or("");
            let (count, last_seen) = crate::service_badge::venue_status_detail(venue);
            json_map(vec![
                ("status", Value::String("ok".into())),
                ("crowd_size", Value::Number(Number::from(count))),
                ("last_seen", Value::Number(Number::from(last_seen))),
            ])
        }
        "gateway.venue_register" => {
            let obj = req.params.as_value();
            let venue = obj.get("venue_id").and_then(|v| v.as_str()).unwrap_or("");
            if venue.is_empty() {
                return Err(rpc_error(-32602, "venue_id required"));
            }
            let token = crate::service_badge::register_venue(venue);
            let exp = u64::from_str_radix(&token, 16).unwrap_or(0);
            json_map(vec![
                ("status", Value::String("ok".into())),
                ("venue_id", Value::String(venue.to_string())),
                ("token", Value::String(token)),
                ("expires_at", Value::Number(Number::from(exp))),
            ])
        }
        "gateway.venue_rotate" => {
            let obj = req.params.as_value();
            let venue = obj.get("venue_id").and_then(|v| v.as_str()).unwrap_or("");
            if venue.is_empty() {
                return Err(rpc_error(-32602, "venue_id required"));
            }
            let token = crate::service_badge::rotate_venue_token(venue);
            let exp = u64::from_str_radix(&token, 16).unwrap_or(0);
            json_map(vec![
                ("status", Value::String("ok".into())),
                ("venue_id", Value::String(venue.to_string())),
                ("token", Value::String(token)),
                ("expires_at", Value::Number(Number::from(exp))),
            ])
        }
        "gateway.dns_lookup" => gateway::dns::dns_lookup(req.params.as_value()),
        "gateway.mobile_cache_status" => {
            serialize_response(gateway::mobile_cache::status_snapshot())?
        }
        "gateway.mobile_cache_flush" => serialize_response(gateway::mobile_cache::flush_cache())?,
        #[cfg(feature = "telemetry")]
        "telemetry.configure" => {
            #[derive(Deserialize)]
            struct TelemetryConfigure {
                sample_rate: Option<f64>,
                compaction_secs: Option<u64>,
            }
            let cfg: TelemetryConfigure = foundation_serialization::json::from_value(
                req.params.clone().into(),
            )
            .unwrap_or(TelemetryConfigure {
                sample_rate: None,
                compaction_secs: None,
            });
            if let Some(rate) = cfg.sample_rate {
                crate::telemetry::set_sample_rate(rate);
            }
            if let Some(secs) = cfg.compaction_secs {
                crate::telemetry::set_compaction_interval(secs);
            }
            json_map(vec![
                ("status", Value::String("ok".to_string())),
                (
                    "sample_rate_ppm",
                    Value::Number(Number::from(crate::telemetry::sample_rate_ppm())),
                ),
                (
                    "compaction_secs",
                    Value::Number(Number::from(crate::telemetry::compaction_interval_secs())),
                ),
            ])
        }
        #[cfg(not(feature = "telemetry"))]
        "telemetry.configure" => {
            return Err(rpc_error(-32603, "telemetry disabled"));
        }
        #[cfg(feature = "telemetry")]
        "analytics" => {
            let q: analytics::AnalyticsQuery = foundation_serialization::json::from_value(
                req.params.clone().into(),
            )
            .unwrap_or(analytics::AnalyticsQuery {
                domain: String::new(),
            });
            let stats = analytics::analytics(&crate::telemetry::READ_STATS, q);
            foundation_serialization::json::to_value(stats).unwrap()
        }
        "microshard.roots.last" => {
            let n = req.params.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            serialize_response(compute_market::recent_roots(n))?
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
            let stats = guard.mempool_stats(lane);
            json_map(vec![
                ("size", Value::Number(Number::from(stats.size as u64))),
                ("age_p50", Value::Number(Number::from(stats.age_p50))),
                ("age_p95", Value::Number(Number::from(stats.age_p95))),
                ("fee_p50", Value::Number(Number::from(stats.fee_p50))),
                ("fee_p90", Value::Number(Number::from(stats.fee_p90))),
                ("fee_floor", Value::Number(Number::from(stats.fee_floor))),
            ])
        }
        "mempool.qos_event" => {
            let lane = req
                .params
                .get("lane")
                .and_then(|v| v.as_str())
                .unwrap_or("consumer");
            let event = req
                .params
                .get("event")
                .and_then(|v| v.as_str())
                .unwrap_or("warning");
            let fee = req.params.get("fee").and_then(|v| v.as_u64()).unwrap_or(0);
            let floor = req
                .params
                .get("floor")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            #[cfg(feature = "telemetry")]
            {
                use crate::telemetry::{FEE_FLOOR_OVERRIDE_TOTAL, FEE_FLOOR_WARNING_TOTAL};
                let labels = [lane];
                FEE_FLOOR_WARNING_TOTAL
                    .ensure_handle_for_label_values(&labels)
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                if event == "override" {
                    FEE_FLOOR_OVERRIDE_TOTAL
                        .ensure_handle_for_label_values(&labels)
                        .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                        .inc();
                }
                diagnostics::tracing::info!(
                    target: "mempool",
                    lane,
                    event,
                    fee,
                    floor,
                    "wallet fee floor event",
                );
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = (lane, event, fee, floor);
            }
            status_value("ok")
        }
        "net.overlay_status" => {
            let status = net::overlay_status();
            json_map(vec![
                ("backend", Value::String(status.backend)),
                (
                    "active_peers",
                    Value::Number(Number::from(status.active_peers as u64)),
                ),
                (
                    "persisted_peers",
                    Value::Number(Number::from(status.persisted_peers as u64)),
                ),
                (
                    "database_path",
                    status
                        .database_path
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
            ])
        }
        "net.peer_stats" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            let m = net::peer_stats(&pk).ok_or(rpc_error(-32602, "unknown peer"))?;
            json_map(vec![
                ("requests", Value::Number(Number::from(m.requests))),
                ("bytes_sent", Value::Number(Number::from(m.bytes_sent))),
                ("drops", drop_counts_to_value(&m.drops)),
                (
                    "handshake_fail",
                    handshake_fail_counts_to_value(&m.handshake_fail),
                ),
                ("reputation", Value::from(m.reputation.score)),
                (
                    "throttle_reason",
                    m.throttle_reason.map(Value::String).unwrap_or(Value::Null),
                ),
                (
                    "throttled_until",
                    Value::Number(Number::from(m.throttled_until)),
                ),
            ])
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
            net::peer::peer_stats_to_json(&stats)
        }
        "net.peer_stats_reset" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            if net::reset_peer_metrics(&pk) {
                status_value("ok")
            } else {
                return Err(rpc_error(-32602, "unknown peer"));
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
                    Ok(over) => json_map(vec![
                        ("status", Value::String("ok".to_string())),
                        ("overwritten", Value::Bool(over)),
                    ]),
                    Err(e) => {
                        return Err(rpc_error(-32602, io_err_msg(&e)));
                    }
                }
            } else {
                let id = req
                    .params
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pk = parse_overlay_peer_param(id)?;
                match net::export_peer_stats(&pk, path) {
                    Ok(over) => json_map(vec![
                        ("status", Value::String("ok".to_string())),
                        ("overwritten", Value::Bool(over)),
                    ]),
                    Err(e) => {
                        return Err(rpc_error(-32602, io_err_msg(&e)));
                    }
                }
            }
        }
        "net.peer_stats_export_all" => {
            let min_rep = req.params.get("min_reputation").and_then(|v| v.as_f64());
            let active = req.params.get("active_within").and_then(|v| v.as_u64());
            let map = net::peer_stats_map(min_rep, active);
            let mut out = Map::new();
            for (peer, metrics) in map {
                out.insert(peer, peer_metrics_to_value(&metrics));
            }
            Value::Object(out)
        }
        "net.peer_stats_persist" => match net::persist_peer_metrics() {
            Ok(()) => status_value("ok"),
            Err(_) => {
                return Err(rpc_error(-32603, "persist failed"));
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
            let pk = parse_overlay_peer_param(id)?;
            if clear {
                if net::clear_throttle(&pk) {
                    status_value("ok")
                } else {
                    return Err(rpc_error(-32602, "unknown peer"));
                }
            } else {
                net::throttle_peer(&pk, "manual");
                status_value("ok")
            }
        }
        "net.backpressure_clear" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let pk = parse_overlay_peer_param(id)?;
            if net::clear_throttle(&pk) {
                status_value("ok")
            } else {
                return Err(rpc_error(-32602, "unknown peer"));
            }
        }
        "net.reputation_sync" => {
            net::reputation_sync();
            status_value("ok")
        }
        "net.rotate_cert" => {
            #[cfg(feature = "quic")]
            {
                let key = crate::net::load_net_key();
                match crate::net::transport_quic::rotate(&key) {
                    Ok(advert) => {
                        let previous: Vec<String> = advert
                            .previous
                            .iter()
                            .map(|fp| crypto_suite::hex::encode(fp))
                            .collect();
                        json_map(vec![
                            ("status", Value::String("ok".to_string())),
                            (
                                "fingerprint",
                                Value::String(crypto_suite::hex::encode(advert.fingerprint)),
                            ),
                            (
                                "previous",
                                Value::Array(previous.into_iter().map(Value::String).collect()),
                            ),
                        ])
                    }
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        diagnostics::tracing::error!(error = %err, "quic_cert_rotation_failed");
                        #[cfg(not(feature = "telemetry"))]
                        let _ = err;
                        return Err(rpc_error(-32603, "rotation failed"));
                    }
                }
            }
            #[cfg(not(feature = "quic"))]
            {
                return Err(rpc_error(-32601, "quic feature not enabled"));
            }
        }
        "net.key_rotate" => {
            let id = req
                .params
                .get("peer_id")
                .and_then(|v| v.as_str())
                .ok_or(rpc_error(-32602, "invalid params"))?;
            let new_key = req
                .params
                .get("new_key")
                .and_then(|v| v.as_str())
                .ok_or(rpc_error(-32602, "invalid params"))?;
            let sig_hex = req
                .params
                .get("signature")
                .and_then(|v| v.as_str())
                .ok_or(rpc_error(-32602, "invalid params"))?;
            let old_bytes =
                crypto_suite::hex::decode(id).map_err(|_| rpc_error(-32602, "invalid params"))?;
            let new_bytes = crypto_suite::hex::decode(new_key)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let sig_bytes = crypto_suite::hex::decode(sig_hex)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let old_pk: [u8; 32] = old_bytes
                .try_into()
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let new_pk: [u8; 32] = new_bytes
                .try_into()
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let sig_arr: [u8; 64] = sig_bytes
                .try_into()
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let sig = Signature::from_bytes(&sig_arr);
            let vk = VerifyingKey::from_bytes(&old_pk)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            if vk.verify(&new_pk, &sig).is_err() {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .ensure_handle_for_label_values(&["bad_sig"])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                return Err(rpc_error(-32602, "bad signature"));
            }
            if net::rotate_peer_key(&old_pk, new_pk) {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .ensure_handle_for_label_values(&["ok"])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                status_value("ok")
            } else {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PEER_KEY_ROTATE_TOTAL
                    .ensure_handle_for_label_values(&["missing"])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                return Err(rpc_error(-32602, "unknown peer"));
            }
        }
        "net.handshake_failures" => {
            let entries = net::recent_handshake_failures();
            let failures = foundation_serialization::json::to_value(entries).unwrap_or(Value::Null);
            json_map(vec![("failures", failures)])
        }
        "net.quic_stats" => match foundation_serialization::json::to_value(net::quic_stats()) {
            Ok(val) => val,
            Err(e) => {
                #[cfg(feature = "telemetry")]
                diagnostics::tracing::warn!(target: "rpc", error = %e, "failed to serialize quic stats");
                #[cfg(not(feature = "telemetry"))]
                let _ = e;
                return Err(rpc_error(-32603, "serialization error"));
            }
        },
        "net.quic_certs" => {
            match foundation_serialization::json::to_value(net::peer_cert_history()) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(target: "rpc", error = %e, "failed to serialize quic cert history");
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(rpc_error(-32603, "serialization error"));
                }
            }
        }
        "net.quic_certs_refresh" => {
            let refreshed = net::refresh_peer_cert_store_from_disk();
            json_map(vec![("reloaded", Value::Bool(refreshed))])
        }
        "peer.rebate_status" => {
            let peer = req
                .params
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let threshold = req
                .params
                .get("threshold")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pk =
                net::uptime::peer_from_bytes(&crypto_suite::hex::decode(peer).unwrap_or_default())
                    .map_err(|_| rpc_error(-32602, "bad peer"))?;
            let eligible = net::uptime::eligible(&pk, threshold, epoch);
            json_map(vec![("eligible", Value::Bool(eligible))])
        }
        "peer.rebate_claim" => {
            let peer = req
                .params
                .get("peer")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let threshold = req
                .params
                .get("threshold")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let reward = req
                .params
                .get("reward")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pk =
                net::uptime::peer_from_bytes(&crypto_suite::hex::decode(peer).unwrap_or_default())
                    .map_err(|_| rpc_error(-32602, "bad peer"))?;
            let voucher = net::uptime::claim(pk, threshold, epoch, reward).unwrap_or(0);
            json_map(vec![("voucher", Value::Number(Number::from(voucher)))])
        }
        "net.config_reload" => {
            if crate::config::reload() {
                status_value("ok")
            } else {
                return Err(rpc_error(-32603, "reload failed"));
            }
        }
        "kyc.verify" => {
            let user = req
                .params
                .get("user")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match kyc::verify(user) {
                Ok(true) => status_value("verified"),
                Ok(false) => status_value("denied"),
                Err(_) => {
                    return Err(rpc_error(-32080, "kyc failure"));
                }
            }
        }
        "pow.get_template" => {
            // simplistic template: zero prev/merkle
            let tmpl = pow::template([0u8; 32], [0u8; 32], [0u8; 32], 1_000_000, 1, 0);
            json_map(vec![
                (
                    "prev_hash",
                    Value::String(crypto_suite::hex::encode(tmpl.prev_hash)),
                ),
                (
                    "merkle_root",
                    Value::String(crypto_suite::hex::encode(tmpl.merkle_root)),
                ),
                (
                    "checkpoint_hash",
                    Value::String(crypto_suite::hex::encode(tmpl.checkpoint_hash)),
                ),
                ("difficulty", Value::Number(Number::from(tmpl.difficulty))),
                ("base_fee", Value::Number(Number::from(tmpl.base_fee))),
                (
                    "timestamp_millis",
                    Value::Number(Number::from(tmpl.timestamp_millis)),
                ),
            ])
        }
        "pow.submit" => {
            let header_value = req
                .params
                .get("header")
                .ok_or(rpc_error(-32602, "missing header"))?;
            let header_obj = header_value
                .as_object()
                .ok_or(rpc_error(-32602, "header must be an object"))?;
            let parse32 =
                |v: &foundation_serialization::json::Value| -> Result<[u8; 32], RpcError> {
                    let s = v.as_str().ok_or(rpc_error(-32602, "bad hex"))?;
                    let bytes =
                        crypto_suite::hex::decode(s).map_err(|_| rpc_error(-32602, "bad hex"))?;
                    let arr: [u8; 32] =
                        bytes.try_into().map_err(|_| rpc_error(-32602, "bad hex"))?;
                    Ok(arr)
                };
            let prev_hash = parse32(
                header_obj
                    .get("prev_hash")
                    .ok_or(rpc_error(-32602, "missing prev_hash"))?,
            )?;
            let merkle_root = parse32(
                header_obj
                    .get("merkle_root")
                    .ok_or(rpc_error(-32602, "missing merkle_root"))?,
            )?;
            let checkpoint_hash = parse32(
                header_obj
                    .get("checkpoint_hash")
                    .ok_or(rpc_error(-32602, "missing checkpoint_hash"))?,
            )?;
            let nonce = header_obj
                .get("nonce")
                .ok_or(rpc_error(-32602, "missing nonce"))?
                .as_u64()
                .ok_or(rpc_error(-32602, "bad nonce"))?;
            let difficulty = header_obj
                .get("difficulty")
                .ok_or(rpc_error(-32602, "missing difficulty"))?
                .as_u64()
                .ok_or(rpc_error(-32602, "bad difficulty"))?;
            let timestamp = header_obj
                .get("timestamp_millis")
                .ok_or(rpc_error(-32602, "missing timestamp_millis"))?
                .as_u64()
                .ok_or(rpc_error(-32602, "bad timestamp_millis"))?;
            let retune_hint = header_obj
                .get("retune_hint")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i8;
            let hdr = BlockHeader {
                prev_hash,
                merkle_root,
                checkpoint_hash,
                nonce,
                difficulty,
                retune_hint,
                base_fee: header_obj
                    .get("base_fee")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1),
                timestamp_millis: timestamp,
                #[cfg(feature = "quantum")]
                dilithium_pubkey: Vec::new(),
                #[cfg(feature = "quantum")]
                dilithium_sig: Vec::new(),
                l2_roots: Vec::new(),
                l2_sizes: Vec::new(),
                vdf_commit: [0u8; 32],
                vdf_output: [0u8; 32],
                vdf_proof: Vec::new(),
            };
            let hash = hdr.hash();
            let val = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
            if val <= u64::MAX / difficulty.max(1) {
                status_value("accepted")
            } else {
                return Err(rpc_error(-32082, "invalid pow"));
            }
        }
        "consensus.difficulty" => serialize_response(consensus::difficulty(&bc))?,
        "consensus.pos.register" => serialize_response(pos::register(req.params.as_value())?)?,
        "consensus.pos.bond" => serialize_response(pos::bond(req.params.as_value())?)?,
        "consensus.pos.unbond" => serialize_response(pos::unbond(req.params.as_value())?)?,
        "consensus.pos.slash" => serialize_response(pos::slash(req.params.as_value())?)?,
        "light.latest_header" => {
            let guard = bc.lock().unwrap();
            foundation_serialization::json::to_value(light::latest_header(&guard)).unwrap()
        }
        "light.headers" => {
            let start = req
                .params
                .get("start")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(200) as usize;
            let guard = bc.lock().unwrap();
            foundation_serialization::json::to_value(light::headers_since(&guard, start, limit))
                .unwrap()
        }
        "light_client.rebate_status" => {
            let status = {
                let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
                light::rebate_status(&guard)
            };
            let mut val = match foundation_serialization::json::to_value(status) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(
                        target: "rpc",
                        error = %e,
                        "failed to serialize rebate status"
                    );
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(rpc_error(-32603, "serialization error".into()));
                }
            };
            strip_null_last_claim_height(&mut val);
            val
        }
        "light_client.rebate_history" => {
            let relayer = if let Some(hex) = req.params.get("relayer").and_then(|v| v.as_str()) {
                match crypto_suite::hex::decode(hex) {
                    Ok(bytes) => Some(bytes),
                    Err(_) => {
                        return Err(rpc_error(-32602, "invalid relayer id".into()));
                    }
                }
            } else {
                None
            };
            let cursor = req.params.get("cursor").and_then(|v| v.as_u64());
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(25)
                .min(200) as usize;
            let history = {
                let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
                light::rebate_history(&guard, relayer.as_deref(), cursor, limit)
            };
            match foundation_serialization::json::to_value(history) {
                Ok(val) => val,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    diagnostics::tracing::warn!(
                        target: "rpc",
                        error = %e,
                        "failed to serialize rebate history"
                    );
                    #[cfg(not(feature = "telemetry"))]
                    let _ = e;
                    return Err(rpc_error(-32603, "serialization error".into()));
                }
            }
        }
        "rent.escrow.balance" => {
            let esc = RentEscrow::open("rent_escrow.db");
            if let Some(id) = req.params.get("id").and_then(|v| v.as_str()) {
                json_map(vec![(
                    "balance",
                    Value::Number(Number::from(esc.balance(id))),
                )])
            } else if let Some(acct) = req.params.get("account").and_then(|v| v.as_str()) {
                json_map(vec![(
                    "balance",
                    Value::Number(Number::from(esc.balance_account(acct))),
                )])
            } else {
                json_map(vec![("balance", Value::Number(Number::from(0)))])
            }
        }
        "mesh.peers" => json_map(vec![(
            "peers",
            foundation_serialization::json::to_value(range_boost::peers()).unwrap_or(Value::Null),
        )]),
        "inflation.params" => serialize_response(inflation::params(&bc))?,
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
            serialize_response(compute_market::stats(accel))?
        }
        "compute_market.provider_balances" => {
            serialize_response(compute_market::provider_balances())?
        }
        "compute_market.audit" => compute_market::settlement_audit(),
        "compute_market.recent_roots" => {
            let n = req.params.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            serialize_response(compute_market::recent_roots(n))?
        }
        "compute_market.sla_history" => {
            let n = req
                .params
                .get("limit")
                .or_else(|| req.params.get("n"))
                .and_then(|v| v.as_u64())
                .unwrap_or(16) as usize;
            compute_market::sla_history(n)
        }
        "compute_market.scheduler_metrics" => compute_market::scheduler_metrics(),
        "compute_market.scheduler_stats" => compute_market::scheduler_stats(),
        "scheduler.stats" => scheduler::stats(),
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
            serialize_response(compute_market::reputation_get(provider))?
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
            serialize_response(compute_market::job_cancel(job_id))?
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
            serialize_response(net::reputation_show(peer))?
        }
        "net.gossip_status" => {
            if let Some(status) = net::gossip_status() {
                foundation_serialization::json::to_value(status)
                    .unwrap_or_else(|_| Value::Object(Map::new()))
            } else {
                status_value("unavailable")
            }
        }
        "net.dns_verify" => {
            let domain = req
                .params
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            serialize_response(net::dns_verify(domain))?
        }
        "stake.role" => serialize_response(pos::role(req.params.as_value())?)?,
        "config.reload" => {
            let ok = crate::config::reload();
            json_map(vec![("reloaded", Value::Bool(ok))])
        }
        "register_handle" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            match handles.lock() {
                Ok(mut reg) => match identity::register_handle(req.params.as_value(), &mut reg) {
                    Ok(v) => serialize_response(v)?,
                    Err(e) => error_value(e.code()),
                },
                Err(_) => error_value("lock poisoned"),
            }
        }
        "identity.anchor" => {
            let scope = req
                .params
                .get("address")
                .and_then(|v| v.as_str())
                .map(|addr| format!("{}:{}", req.method, addr))
                .unwrap_or_else(|| req.method.clone());
            check_nonce(scope, &req.params, &nonces)?;
            match dids.lock() {
                Ok(mut reg) => {
                    match identity::anchor_did(req.params.as_value(), &mut reg, &NODE_GOV_STORE) {
                        Ok(v) => serialize_response(v)?,
                        Err(e) => error_value(e.code()),
                    }
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "resolve_handle" => match handles.lock() {
            Ok(reg) => serialize_response(identity::resolve_handle(req.params.as_value(), &reg))?,
            Err(_) => serialize_response(identity::HandleResolutionResponse { address: None })?,
        },
        "identity.resolve" => match dids.lock() {
            Ok(reg) => serialize_response(identity::resolve_did(req.params.as_value(), &reg))?,
            Err(_) => serialize_response(identity::DidResolutionResponse {
                address: String::new(),
                document: None,
                hash: None,
                nonce: None,
                updated_at: None,
                public_key: None,
                remote_attestation: None,
            })?,
        },
        "whoami" => match handles.lock() {
            Ok(reg) => serialize_response(identity::whoami(req.params.as_value(), &reg))?,
            Err(_) => serialize_response(identity::WhoAmIResponse {
                address: String::new(),
                handle: None,
            })?,
        },
        "record_le_request" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
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
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_request(&base, agency, case, jurisdiction, lang)
                    {
                        Ok(_) => status_value("ok"),
                        Err(_) => error_value("io"),
                    }
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "warrant_canary" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let msg = req
                .params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    match crate::le_portal::record_canary(&base, msg) {
                        Ok(hash) => json_map(vec![("hash", Value::String(hash))]),
                        Err(_) => error_value("io"),
                    }
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "le.list_requests" => match bc.lock() {
            Ok(guard) => {
                let base = guard.path.clone();
                match crate::le_portal::list_requests(&base) {
                    Ok(v) => foundation_serialization::json::to_value(v).unwrap_or_default(),
                    Err(_) => error_value("io"),
                }
            }
            Err(_) => error_value("lock poisoned"),
        },
        "le.record_action" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
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
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_action(&base, agency, action, jurisdiction, lang)
                    {
                        Ok(hash) => json_map(vec![("hash", Value::String(hash))]),
                        Err(_) => error_value("io"),
                    }
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "le.upload_evidence" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let agency = req
                .params
                .get("agency")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let case_id = req
                .params
                .get("case_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let data_b64 = req
                .params
                .get("evidence")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let data = match decode_standard(data_b64) {
                Ok(d) => d,
                Err(_) => return Ok(error_value("decode")),
            };
            match bc.lock() {
                Ok(guard) => {
                    let base = guard.path.clone();
                    let jurisdiction = guard.config.jurisdiction.as_deref().unwrap_or("UNSPEC");
                    let lang = req
                        .params
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en");
                    match crate::le_portal::record_evidence(
                        &base,
                        agency,
                        case_id,
                        jurisdiction,
                        lang,
                        &data,
                    ) {
                        Ok(hash) => json_map(vec![("hash", Value::String(hash))]),
                        Err(_) => error_value("io"),
                    }
                }
                Err(_) => error_value("lock poisoned"),
            }
        }
        "service_badge_issue" => match bc.lock() {
            Ok(mut guard) => {
                let token = guard.badge_tracker_mut().force_issue();
                json_map(vec![("badge", Value::String(token))])
            }
            Err(_) => error_value("lock poisoned"),
        },
        "service_badge_revoke" => match bc.lock() {
            Ok(mut guard) => {
                guard.badge_tracker_mut().revoke();
                json_map(vec![("revoked", Value::Bool(true))])
            }
            Err(_) => error_value("lock poisoned"),
        },
        "service_badge_verify" => {
            let badge = req
                .params
                .get("badge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            json_map(vec![(
                "valid",
                Value::Bool(crate::service_badge::verify(badge)),
            )])
        }
        "submit_tx" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let tx_hex = req.params.get("tx").and_then(|v| v.as_str()).unwrap_or("");
            match crypto_suite::hex::decode(tx_hex)
                .ok()
                .and_then(|b| binary::decode::<SignedTransaction>(&b).ok())
            {
                Some(mut tx) => {
                    if let Some(f) = req.params.get("max_fee").and_then(|v| v.as_u64()) {
                        tx.payload.fee = f;
                    }
                    if let Some(t) = req.params.get("tip").and_then(|v| v.as_u64()) {
                        tx.tip = t;
                    }
                    match bc.lock() {
                        Ok(mut guard) => match guard.submit_transaction(tx) {
                            Ok(()) => status_value("ok"),
                            Err(e) => error_value(format!("{e:?}")),
                        },
                        Err(_) => error_value("lock poisoned"),
                    }
                }
                None => return Err(rpc_error(-32602, "invalid params")),
            }
        }
        "set_snapshot_interval" => {
            let interval = req
                .params
                .get("interval")
                .and_then(|v| v.as_u64())
                .ok_or(rpc_error(-32602, "invalid params"))?;
            if interval < 10 {
                return Err(SnapshotError::IntervalTooSmall.into());
            }
            if let Ok(mut guard) = bc.lock() {
                guard.snapshot.set_interval(interval);
                guard.config.snapshot_interval = interval;
                guard.save_config();
            } else {
                return Err(rpc_error(-32603, "lock poisoned"));
            }
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::SNAPSHOT_INTERVAL.set(interval as i64);
                crate::telemetry::SNAPSHOT_INTERVAL_CHANGED.set(interval as i64);
            }
            #[cfg(feature = "telemetry")]
            diagnostics::log::info!("snapshot_interval_changed {interval}");
            status_value("ok")
        }
        "start_mining" => {
            if runtime_cfg.relay_only {
                let mut inner = Map::new();
                inner.insert("code".to_string(), Value::Number(Number::from(-32075)));
                inner.insert(
                    "message".to_string(),
                    Value::String("relay_only".to_string()),
                );
                json_map(vec![("error", Value::Object(inner))])
            } else {
                check_nonce(req.method.as_str(), &req.params, &nonces)?;
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
                status_value("ok")
            }
        }
        "stop_mining" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            mining.store(false, Ordering::SeqCst);
            status_value("ok")
        }
        "jurisdiction.status" => serialize_response(jurisdiction::status(&bc)?)?,
        "jurisdiction.set" => {
            check_nonce(req.method.as_str(), &req.params, &nonces)?;
            let path = req
                .params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or(rpc_error(-32072, "missing path"))?;
            serialize_response(jurisdiction::set(&bc, path)?)?
        }
        "jurisdiction.policy_diff" => {
            let path = req
                .params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or(rpc_error(-32072, "missing path"))?;
            jurisdiction::policy_diff(&bc, path)?
        }
        "metrics" => {
            #[cfg(feature = "telemetry")]
            {
                let m = crate::gather_metrics().unwrap_or_default();
                foundation_serialization::json::to_value(m).unwrap_or(Value::Null)
            }
            #[cfg(not(feature = "telemetry"))]
            {
                Value::String("telemetry disabled".to_string())
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
                Some((p25, median, p75)) => json_map(vec![
                    ("p25", Value::Number(Number::from(p25))),
                    ("median", Value::Number(Number::from(median))),
                    ("p75", Value::Number(Number::from(p75))),
                ]),
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
            status_value("ok")
        }
        "compute_cancel_arm" => {
            crate::compute_market::settlement::Settlement::cancel_arm();
            status_value("ok")
        }
        "compute_back_to_dry_run" => {
            let reason = req
                .params
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            crate::compute_market::settlement::Settlement::back_to_dry_run(reason);
            status_value("ok")
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
                Err(_) => return Err(rpc_error(-32002, "release failed")),
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
                foundation_serialization::json::to_value(proof)
                    .map_err(|_| rpc_error(-32603, "internal error"))?
            } else {
                return Err(rpc_error(-32003, "not found"));
            }
        }
        "htlc_status" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            serialize_response(htlc::status(id))?
        }
        "htlc_refund" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let now = req.params.get("now").and_then(|v| v.as_u64()).unwrap_or(0);
            serialize_response(htlc::refund(id, now))?
        }
        "energy.register_provider" => energy::register(&req.params),
        "energy.market_state" => {
            let provider = req
                .params
                .get("provider_id")
                .and_then(|value| value.as_str());
            energy::market_state(provider)
        }
        "energy.receipts" => energy::receipts(&req.params),
        "energy.credits" => energy::credits(&req.params),
        "energy.disputes" => energy::disputes(&req.params),
        "energy.settle" => {
            let height = bc
                .lock()
                .map(|guard| guard.block_height)
                .unwrap_or_default();
            energy::settle(&req.params, height)
        }
        "energy.submit_reading" => {
            let height = bc
                .lock()
                .map(|guard| guard.block_height)
                .unwrap_or_default();
            energy::submit_reading(&req.params, height)
        }
        "energy.flag_dispute" => {
            let height = bc
                .lock()
                .map(|guard| guard.block_height)
                .unwrap_or_default();
            energy::flag_dispute(&req.params, height)
        }
        "energy.resolve_dispute" => {
            let height = bc
                .lock()
                .map(|guard| guard.block_height)
                .unwrap_or_default();
            energy::resolve_dispute(&req.params, height)
        }
        "storage_upload" => {
            let object_id = req
                .params
                .get("object_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let provider_id = req
                .params
                .get("provider_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let original_bytes = req
                .params
                .get("original_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let shares = req
                .params
                .get("shares")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
            let price_per_block = req
                .params
                .get("price_per_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let start_block = req
                .params
                .get("start_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let retention_blocks = req
                .params
                .get("retention_blocks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let chunk_count = shares.max(1) as usize;
            let chunks = deterministic_chunks_for_object(&object_id, chunk_count);
            let tree = merkle_tree_from_chunks(&chunks);
            let contract = StorageContract {
                object_id: object_id.clone(),
                provider_id: provider_id.clone(),
                original_bytes,
                shares,
                price_per_block,
                start_block,
                retention_blocks,
                next_payment_block: start_block + 1,
                accrued: 0,
                total_deposit: 0,
                last_payment_block: None,
                storage_root: tree.root,
            };
            let offer = StorageOffer::new(
                provider_id,
                original_bytes,
                price_per_block,
                retention_blocks,
            );
            storage::upload(contract, vec![offer])
        }
        "storage_challenge" => {
            let object_id = req
                .params
                .get("object_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let provider_id = req.params.get("provider_id").and_then(|v| v.as_str());
            let chunk_idx = req
                .params
                .get("chunk_idx")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let chunk_data = req
                .params
                .get("chunk_data")
                .and_then(|v| v.as_str())
                .map(|s| crypto_suite::hex::decode(s).unwrap_or_default())
                .unwrap_or_default();
            let proof_bytes = req
                .params
                .get("proof")
                .and_then(|v| v.as_str())
                .map(|s| crypto_suite::hex::decode(s).unwrap_or_default())
                .unwrap_or_else(|| vec![0u8; 32]);
            let proof = match MerkleProof::new(proof_bytes) {
                Ok(proof) => proof,
                Err(err) => return Ok(error_value(format!("invalid proof: {err}"))),
            };
            let current_block = req
                .params
                .get("current_block")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            storage::challenge(
                object_id,
                provider_id,
                chunk_idx,
                chunk_data.as_slice(),
                &proof,
                current_block,
            )
        }
        "storage.manifests" => {
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            storage::manifest_summaries(limit)
        }
        "storage_provider_profiles" => storage::provider_profiles(),
        "storage_provider_set_maintenance" => {
            let provider = req
                .params
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let maintenance = req
                .params
                .get("maintenance")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            storage::set_provider_maintenance(provider, maintenance)
        }
        "storage_incentives" => storage::incentives_snapshot(),
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
            serialize_response(governance::gov_propose(
                &NODE_GOV_STORE,
                proposer,
                key,
                new_value,
                min,
                max,
                epoch,
                deadline,
            )?)?
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
            serialize_response(governance::gov_vote(
                &NODE_GOV_STORE,
                voter,
                pid,
                choice,
                epoch,
            )?)?
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
            serialize_response(governance::submit_proposal(
                &NODE_GOV_STORE,
                proposer,
                key,
                new_value,
                min,
                max,
                deps,
                epoch,
                deadline,
            )?)?
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
            serialize_response(governance::vote_proposal(
                &NODE_GOV_STORE,
                voter,
                pid,
                choice,
                epoch,
            )?)?
        }
        "gov.treasury.disbursements" => {
            let params = parse_params::<treasury::TreasuryDisbursementsRequest>(&req.params)?;
            serialize_response(treasury::disbursements(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.submit_disbursement" => {
            let params = parse_params::<treasury::SubmitDisbursementRequest>(&req.params)?;
            serialize_response(treasury::submit_disbursement(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.disbursement" => {
            let params = parse_params::<treasury::GetDisbursementRequest>(&req.params)?;
            serialize_response(treasury::get_disbursement(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.queue_disbursement" => {
            let mut params = parse_params::<treasury::QueueDisbursementRequest>(&req.params)?;
            if params.current_epoch == 0 {
                let epoch = bc
                    .lock()
                    .map(|guard| guard.block_height / EPOCH_BLOCKS)
                    .unwrap_or_default();
                params.current_epoch = epoch;
            }
            serialize_response(treasury::queue_disbursement(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.execute_disbursement" => {
            let params = parse_params::<treasury::ExecuteDisbursementRequest>(&req.params)?;
            serialize_response(treasury::execute_disbursement(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.rollback_disbursement" => {
            let params = parse_params::<treasury::RollbackDisbursementRequest>(&req.params)?;
            serialize_response(treasury::rollback_disbursement(&NODE_GOV_STORE, params)?)?
        }
        "gov.treasury.balance" => serialize_response(treasury::balance(&NODE_GOV_STORE)?)?,
        "gov.treasury.balance_history" => {
            let params = parse_params::<treasury::TreasuryBalanceHistoryRequest>(&req.params)?;
            serialize_response(treasury::balance_history(&NODE_GOV_STORE, params)?)?
        }
        "gov.release_signers" => serialize_response(governance::release_signers(&NODE_GOV_STORE)?)?,
        "gov_list" => serialize_response(governance::gov_list(&NODE_GOV_STORE)?)?,
        "gov_params" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let params = GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
            serialize_response(governance::gov_params(&params, epoch)?)?
        }
        "gov_rollback_last" => {
            let epoch = req
                .params
                .get("epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let mut params = GOV_PARAMS.lock().unwrap_or_else(|e| e.into_inner());
            let mut chain = bc.lock().unwrap_or_else(|e| e.into_inner());
            let mut rt = crate::governance::Runtime::new(&mut *chain);
            if let Some(handle) = market.clone() {
                rt.set_market(handle);
            }
            if let Some(handle) = readiness_handle.clone() {
                rt.set_ad_readiness(handle);
            }
            serialize_response(governance::gov_rollback_last(
                &NODE_GOV_STORE,
                &mut params,
                &mut rt,
                epoch,
            )?)?
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
            let mut rt = crate::governance::Runtime::new(&mut *chain);
            if let Some(handle) = market.clone() {
                rt.set_market(handle);
            }
            if let Some(handle) = readiness_handle.clone() {
                rt.set_ad_readiness(handle);
            }
            serialize_response(governance::gov_rollback(
                &NODE_GOV_STORE,
                id,
                &mut params,
                &mut rt,
                epoch,
            )?)?
        }
        "vm.estimate_gas" => {
            let code_hex = req
                .params
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = crypto_suite::hex::decode(code_hex)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let gas = vm::estimate_gas(code);
            json_map(vec![("gas_used", Value::Number(Number::from(gas)))])
        }
        "vm.exec_trace" => {
            let code_hex = req
                .params
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = crypto_suite::hex::decode(code_hex)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            let trace = vm::exec_trace(code);
            foundation_serialization::json::to_value(trace)
                .unwrap_or_else(|_| Value::Object(Map::new()))
        }
        "vm.storage_read" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data = vm::storage_read(id).unwrap_or_default();
            json_map(vec![(
                "data",
                Value::String(crypto_suite::hex::encode(data)),
            )])
        }
        "vm.storage_write" => {
            let id = req.params.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data_hex = req
                .params
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bytes = crypto_suite::hex::decode(data_hex)
                .map_err(|_| rpc_error(-32602, "invalid params"))?;
            vm::storage_write(id, bytes);
            status_value("ok")
        }
        _ => return Err(rpc_error(-32601, "method not found")),
    })
}

pub async fn run_rpc_server_with_market(
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    market: Option<MarketplaceHandle>,
    readiness: Option<crate::ad_readiness::AdReadinessHandle>,
    governor: Option<Arc<launch_governor::GovernorHandle>>,
    addr: String,
    cfg: RpcConfig,
    ready: oneshot::Sender<String>,
) -> std::io::Result<()> {
    let bind_addr: SocketAddr = addr.parse().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid rpc bind address {addr}: {err}"),
        )
    })?;
    let listener =
        net::listener::bind_runtime("rpc", "rpc_listener_bind_failed", bind_addr).await?;
    let local = listener.local_addr()?.to_string();
    let _ = ready.send(local);

    let nonces = Arc::new(Mutex::new(HashSet::<(String, u64)>::new()));
    let handles = Arc::new(Mutex::new(HandleRegistry::open("identity_db")));
    let did_path = DidRegistry::default_path();
    let dids = Arc::new(Mutex::new(DidRegistry::open(&did_path)));
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

    let state = RpcState {
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg: Arc::clone(&runtime_cfg),
        market,
        ad_readiness: readiness,
        governor,
        clients,
        tokens_per_sec,
        ban_secs,
        client_timeout,
        concurrent: Arc::new(Semaphore::new(1024)),
    };

    let router = Router::new(state)
        .route(Method::Options, "/", handle_rpc_options)
        .route(Method::Post, "/", handle_rpc_post)
        .get("/logs/search", handle_logs_search)
        .upgrade("/logs/tail", handle_logs_tail)
        .upgrade("/vm/trace", handle_vm_trace)
        .upgrade("/state_stream", handle_state_stream)
        .get("/badge/status", handle_badge_status)
        .get("/dashboard", handle_dashboard);

    let mut server_cfg = ServerConfig::default();
    server_cfg.request_timeout = runtime_cfg.request_timeout;
    server_cfg.max_body_bytes = runtime_cfg.max_body_bytes;
    serve(listener, router, server_cfg).await
}

pub async fn run_rpc_server(
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    addr: String,
    cfg: RpcConfig,
    ready: oneshot::Sender<String>,
) -> std::io::Result<()> {
    run_rpc_server_with_market(bc, mining, None, None, None, addr, cfg, ready).await
}

#[cfg(any(test, feature = "fuzzy", feature = "integration-tests"))]
pub fn fuzz_runtime_config() -> Arc<RpcRuntimeConfig> {
    fuzz_runtime_config_with_overrides(false, None)
}

#[cfg(any(test, feature = "integration-tests"))]
pub fn fuzz_runtime_config_with_admin(token: impl Into<String>) -> Arc<RpcRuntimeConfig> {
    fuzz_runtime_config_with_overrides(true, Some(token.into()))
}

#[cfg(any(test, feature = "fuzzy", feature = "integration-tests"))]
fn fuzz_runtime_config_with_overrides(
    enable_debug: bool,
    admin_token: Option<String>,
) -> Arc<RpcRuntimeConfig> {
    Arc::new(RpcRuntimeConfig {
        allowed_hosts: vec!["localhost".into()],
        cors_allow_origins: Vec::new(),
        max_body_bytes: 1024,
        request_timeout: Duration::from_secs(1),
        enable_debug,
        admin_token,
        relay_only: false,
    })
}

#[cfg(any(test, feature = "fuzzy", feature = "integration-tests"))]
pub fn fuzz_dispatch_request(
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
    market: Option<MarketplaceHandle>,
    readiness: Option<crate::ad_readiness::AdReadinessHandle>,
    request: RpcRequest,
    auth_header: Option<String>,
    peer_ip: Option<IpAddr>,
) -> RpcResponse {
    let state = RpcState {
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg: Arc::clone(&runtime_cfg),
        market,
        ad_readiness: readiness,
        governor: None,
        clients: Arc::new(Mutex::new(HashMap::new())),
        tokens_per_sec: 128.0,
        ban_secs: 1,
        client_timeout: 1,
        concurrent: Arc::new(Semaphore::new(64)),
    };

    execute_rpc(&state, request, auth_header.as_deref(), peer_ip)
}
