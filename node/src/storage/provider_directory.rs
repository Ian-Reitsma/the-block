#![forbid(unsafe_code)]

#[cfg(feature = "quic")]
use crate::net::send_quic_msg;
use crate::net::{load_net_key, send_msg, Transport};
use crate::net::peer::known_peers_with_info;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_PROVIDER_ADVERT_SEEN_TOTAL, STORAGE_PROVIDER_CANDIDATE_GAUGE,
    STORAGE_PROVIDER_DISCOVERY_LATENCY_SECONDS, STORAGE_PROVIDER_PUBLISH_TOTAL,
    STORAGE_PROVIDER_STALE_REJECT_TOTAL,
};
use concurrency::{mutex, Bytes, MutexExt, MutexT};
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use rand::RngCore;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use std::convert::TryFrom;
use storage_market::{DiscoveryRequest, ProviderDirectory, ProviderProfile, StorageMarket};

type StorageMarketHandle = Arc<MutexT<StorageMarket>>;

const LOOKUP_MAX_AGE_SECS: u64 = 30;
const LOOKUP_RATE_LIMIT_SECS: u64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderAdvertisement {
    pub profile: ProviderProfile,
    pub version: u64,
    pub ttl_secs: u64,
    pub expires_at: u64,
    pub publisher: [u8; 32],
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Bytes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderLookupRequest {
    pub request: DiscoveryRequest,
    pub nonce: u64,
    pub issued_at: u64,
    pub ttl: u8,
    pub origin: [u8; 32],
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Bytes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderLookupResponse {
    pub nonce: u64,
    pub responder: [u8; 32],
    pub providers: Vec<ProviderProfile>,
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Bytes,
}

struct ProviderAdvertisementBody {
    profile: ProviderProfile,
    version: u64,
    ttl_secs: u64,
    expires_at: u64,
    publisher: [u8; 32],
}

struct ProviderLookupBody<'a> {
    request: &'a DiscoveryRequest,
    nonce: u64,
    issued_at: u64,
    ttl: u8,
    origin: [u8; 32],
}

struct ProviderLookupResponseBody<'a> {
    nonce: u64,
    responder: [u8; 32],
    providers: &'a [ProviderProfile],
}

impl ProviderAdvertisement {
    pub fn sign(profile: ProviderProfile, ttl_secs: u64, sk: &SigningKey) -> Self {
        let expires_at = now().saturating_add(ttl_secs);
        let mut body = ProviderAdvertisementBody {
            profile,
            version: 0,
            ttl_secs,
            expires_at,
            publisher: sk.verifying_key().to_bytes(),
        };
        body.version = body.profile.version.max(1);
        let bytes = serialize_body(&body);
        let sig = sk.sign(&bytes);
        Self {
            profile: body.profile,
            version: body.version,
            ttl_secs,
            expires_at,
            publisher: body.publisher,
            signature: Bytes::from(sig.to_bytes().to_vec()),
        }
    }

    pub fn verify(&self) -> bool {
        if self.expires_at <= now() {
            return false;
        }
        let vk = match VerifyingKey::from_bytes(&self.publisher) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let body = ProviderAdvertisementBody {
            profile: self.profile.clone(),
            version: self.version,
            ttl_secs: self.ttl_secs,
            expires_at: self.expires_at,
            publisher: self.publisher,
        };
        let bytes = serialize_body(&body);
        let sig_bytes: [u8; crypto_suite::signatures::ed25519::SIGNATURE_LENGTH] =
            match TryFrom::try_from(self.signature.as_ref()) {
                Ok(arr) => arr,
                Err(_) => return false,
            };
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(&bytes, &sig).is_ok()
    }
}

impl ProviderLookupRequest {
    pub fn sign(request: DiscoveryRequest, ttl: u8, sk: &SigningKey) -> Self {
        let mut nonce_bytes = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = u64::from_le_bytes(nonce_bytes);
        let body = ProviderLookupBody {
            request: &request,
            nonce,
            issued_at: now(),
            ttl,
            origin: sk.verifying_key().to_bytes(),
        };
        let bytes = serialize_lookup_body(&body);
        let sig = sk.sign(&bytes);
        Self {
            request: request.clone(),
            nonce,
            issued_at: body.issued_at,
            ttl,
            origin: body.origin,
            signature: Bytes::from(sig.to_bytes().to_vec()),
        }
    }

    pub fn verify(&self) -> bool {
        if now().saturating_sub(self.issued_at) > LOOKUP_MAX_AGE_SECS {
            return false;
        }
        let vk = match VerifyingKey::from_bytes(&self.origin) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let body = ProviderLookupBody {
            request: &self.request,
            nonce: self.nonce,
            issued_at: self.issued_at,
            ttl: self.ttl,
            origin: self.origin,
        };
        let bytes = serialize_lookup_body(&body);
        let sig_bytes: [u8; crypto_suite::signatures::ed25519::SIGNATURE_LENGTH] =
            match TryFrom::try_from(self.signature.as_ref()) {
                Ok(arr) => arr,
                Err(_) => return false,
            };
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(&bytes, &sig).is_ok()
    }
}

impl ProviderLookupResponse {
    pub fn sign(nonce: u64, providers: Vec<ProviderProfile>, sk: &SigningKey) -> Self {
        let body = ProviderLookupResponseBody {
            nonce,
            responder: sk.verifying_key().to_bytes(),
            providers: &providers,
        };
        let bytes = serialize_lookup_response_body(&body);
        let sig = sk.sign(&bytes);
        Self {
            nonce,
            responder: body.responder,
            providers,
            signature: Bytes::from(sig.to_bytes().to_vec()),
        }
    }

    pub fn verify(&self) -> bool {
        let vk = match VerifyingKey::from_bytes(&self.responder) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let body = ProviderLookupResponseBody {
            nonce: self.nonce,
            responder: self.responder,
            providers: &self.providers,
        };
        let bytes = serialize_lookup_response_body(&body);
        let sig_bytes: [u8; crypto_suite::signatures::ed25519::SIGNATURE_LENGTH] =
            match TryFrom::try_from(self.signature.as_ref()) {
                Ok(arr) => arr,
                Err(_) => return false,
            };
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(&bytes, &sig).is_ok()
    }
}

pub struct NetworkProviderDirectory {
    market: StorageMarketHandle,
    cache: MutexT<HashMap<String, ProviderProfile>>,
    ttl_secs: u64,
    seen_publishers: MutexT<HashSet<[u8; 32]>>,
    seen_requests: MutexT<HashSet<(u64, [u8; 32])>>,
    seen_responses: MutexT<HashSet<(u64, [u8; 32])>>,
    last_request_from_origin: MutexT<HashMap<[u8; 32], u64>>,
}

impl NetworkProviderDirectory {
    pub fn new(market: StorageMarketHandle, ttl_secs: u64) -> Self {
        Self {
            market,
            cache: mutex(HashMap::new()),
            ttl_secs,
            seen_publishers: mutex(HashSet::new()),
            seen_requests: mutex(HashSet::new()),
            seen_responses: mutex(HashSet::new()),
            last_request_from_origin: mutex(HashMap::new()),
        }
    }

    pub fn ingest_advertisement(&self, advert: ProviderAdvertisement) {
        if advert.expires_at <= now() || !advert.verify() {
            #[cfg(feature = "telemetry")]
            STORAGE_PROVIDER_STALE_REJECT_TOTAL.inc();
            return;
        }
        let mut profile = advert.profile.clone();
        profile.mark_version(advert.version);
        profile.set_expiry(advert.expires_at);
        if let Some(updated) = self.cache_profile(profile.clone()) {
            self.track_publisher(advert.publisher);
            let _ = self.market.guard().cache_provider_profile(updated);
            #[cfg(feature = "telemetry")]
            STORAGE_PROVIDER_ADVERT_SEEN_TOTAL.inc();
        } else {
            #[cfg(feature = "telemetry")]
            STORAGE_PROVIDER_STALE_REJECT_TOTAL.inc();
        }
    }

    pub fn ingest_lookup_request(
        &self,
        request: ProviderLookupRequest,
        responder: Option<SocketAddr>,
    ) {
        if !request.verify() {
            return;
        }
        {
            let mut guard = self.seen_requests.guard();
            if guard.contains(&(request.nonce, request.origin)) {
                return;
            }
            guard.insert((request.nonce, request.origin));
        }
        if self.rate_limited(request.origin, request.issued_at) {
            return;
        }

        let matches = self.discover(&request.request).unwrap_or_default();
        if let Some(addr) = responder {
            self.send_lookup_response(request.nonce, matches.clone(), addr);
        }

        if request.ttl > 0 {
            self.forward_lookup(request);
        }
    }

    pub fn ingest_lookup_response(&self, response: ProviderLookupResponse) {
        if !response.verify() {
            return;
        }
        {
            let mut guard = self.seen_responses.guard();
            if guard.contains(&(response.nonce, response.responder)) {
                return;
            }
            guard.insert((response.nonce, response.responder));
        }
        for profile in response.providers {
            let _ = self.cache_profile(profile.clone());
            let _ = self.market.guard().cache_provider_profile(profile);
        }
    }

    fn rate_limited(&self, origin: [u8; 32], issued_at: u64) -> bool {
        let mut guard = self.last_request_from_origin.guard();
        let last = guard.get(&origin).copied().unwrap_or(0);
        if issued_at.saturating_sub(last) < LOOKUP_RATE_LIMIT_SECS {
            return true;
        }
        guard.insert(origin, issued_at);
        false
    }

    fn track_publisher(&self, publisher: [u8; 32]) {
        let mut guard = self.seen_publishers.guard();
        guard.insert(publisher);
    }

    fn cache_profile(&self, profile: ProviderProfile) -> Option<ProviderProfile> {
        let mut guard = self.cache.guard();
        let entry = guard.get(&profile.provider_id).cloned();
        let newer = entry
            .as_ref()
            .map(|p| {
                profile.version > p.version
                    || (profile.version == p.version
                        && profile.expires_at.unwrap_or(0) > p.expires_at.unwrap_or(0))
            })
            .unwrap_or(true);
        if newer {
            guard.insert(profile.provider_id.clone(), profile.clone());
            Some(profile)
        } else {
            None
        }
    }

    fn publish_profile(&self, profile: ProviderProfile) {
        let sk = load_net_key();
        let advert = ProviderAdvertisement::sign(profile, self.ttl_secs, &sk);
        if let Some(updated) = self.cache_profile(advert.profile.clone()) {
            let _ = self.market.guard().cache_provider_profile(updated);
        }
        let payload = crate::net::Message::new(
            crate::net::Payload::StorageProviderAdvertisement(advert),
            &sk,
        );
        let label = if let Ok(msg) = payload {
            self.broadcast(msg);
            "ok"
        } else {
            "error"
        };
        #[cfg(feature = "telemetry")]
        {
            let labels = [label];
            STORAGE_PROVIDER_PUBLISH_TOTAL
                .with_label_values(&labels)
                .inc();
        }
    }

    fn broadcast(&self, msg: crate::net::Message) {
        for (addr, transport, cert) in known_peers_with_info() {
            match transport {
                Transport::Tcp => {
                    let _ = send_msg(addr, &msg);
                }
                Transport::Quic => {
                    #[cfg(feature = "quic")]
                    if let Some(c) = cert.as_ref() {
                        let _ = send_quic_msg(addr, c, &msg);
                    }
                    #[cfg(not(feature = "quic"))]
                    let _ = send_msg(addr, &msg);
                }
            }
        }
    }
}

impl ProviderDirectory for NetworkProviderDirectory {
    fn publish(&self, profile: ProviderProfile) {
        self.publish_profile(profile);
    }

    fn discover(&self, request: &DiscoveryRequest) -> storage_market::Result<Vec<ProviderProfile>> {
        #[cfg(feature = "telemetry")]
        let started = Instant::now();
        let mut out = Vec::new();
        let now = now();
        let mut guard = self.cache.guard();
        guard.retain(|_, profile| !profile.is_expired(now));
        for profile in guard.values() {
            if profile.max_capacity_bytes < request.required_capacity_bytes() {
                continue;
            }
            if let Some(region) = &request.region {
                if profile.region.as_deref() != Some(region.as_str()) {
                    continue;
                }
            }
            if let Some(max_price) = request.max_price_per_block {
                if profile.price_per_block > max_price {
                    continue;
                }
            }
            if let Some(min_success) = request.min_success_rate_ppm {
                if profile.success_rate_ppm() < min_success {
                    continue;
                }
            }
            out.push(profile.clone());
        }
        #[cfg(feature = "telemetry")]
        {
            STORAGE_PROVIDER_CANDIDATE_GAUGE.set(out.len() as i64);
            STORAGE_PROVIDER_DISCOVERY_LATENCY_SECONDS.observe(started.elapsed().as_secs_f64());
        }
        if out.is_empty() {
            self.broadcast_lookup(request.clone());
        }
        Ok(out)
    }
}

type DirectoryHandle = Arc<MutexT<Option<Arc<NetworkProviderDirectory>>>>;

static DIRECTORY: concurrency::Lazy<DirectoryHandle> =
    concurrency::Lazy::new(|| Arc::new(mutex(None)));

pub fn install_directory(market: StorageMarketHandle) {
    let dir = Arc::new(NetworkProviderDirectory::new(market.clone(), 15 * 60));
    if let Ok(profiles) = market.guard().provider_profiles() {
        for profile in profiles {
            let _ = dir.cache_profile(profile.clone());
        }
    }
    *DIRECTORY.guard() = Some(dir.clone());
    storage_market::install_provider_directory(dir);
}

pub fn directory() -> Option<Arc<NetworkProviderDirectory>> {
    DIRECTORY.guard().clone()
}

pub fn handle_advertisement(advert: ProviderAdvertisement) {
    if let Some(dir) = directory() {
        dir.ingest_advertisement(advert);
    }
}

pub fn handle_lookup_request(request: ProviderLookupRequest, responder: Option<SocketAddr>) {
    if let Some(dir) = directory() {
        dir.ingest_lookup_request(request, responder);
    }
}

pub fn handle_lookup_response(response: ProviderLookupResponse) {
    if let Some(dir) = directory() {
        dir.ingest_lookup_response(response);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn serialize_body(body: &ProviderAdvertisementBody) -> Vec<u8> {
    use foundation_serialization::json::Number;
    let mut map = foundation_serialization::json::Map::new();
    map.insert(
        "profile".to_string(),
        foundation_serialization::json::value_from_slice(
            &storage_market::codec::serialize_provider_profile(&body.profile).unwrap_or_default(),
        )
        .unwrap_or(foundation_serialization::json::Value::Null),
    );
    map.insert(
        "version".to_string(),
        foundation_serialization::json::Value::Number(Number::from(body.version)),
    );
    map.insert(
        "ttl_secs".to_string(),
        foundation_serialization::json::Value::Number(Number::from(body.ttl_secs)),
    );
    map.insert(
        "expires_at".to_string(),
        foundation_serialization::json::Value::Number(Number::from(body.expires_at)),
    );
    map.insert(
        "publisher".to_string(),
        foundation_serialization::json::Value::Array(
            body.publisher
                .iter()
                .copied()
                .map(|byte| {
                    foundation_serialization::json::Value::Number(Number::from(byte as u64))
                })
                .collect(),
        ),
    );
    foundation_serialization::json::to_vec_value(&foundation_serialization::json::Value::Object(
        map,
    ))
}

fn serialize_lookup_body(body: &ProviderLookupBody<'_>) -> Vec<u8> {
    let mut map = foundation_serialization::json::Map::new();
    let mut req = foundation_serialization::json::Map::new();
    req.insert(
        "object_size".into(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.request.object_size),
        ),
    );
    req.insert(
        "shares".into(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.request.shares),
        ),
    );
    req.insert(
        "region".into(),
        body.request
            .region
            .as_ref()
            .map(|r| foundation_serialization::json::Value::String(r.clone()))
            .unwrap_or(foundation_serialization::json::Value::Null),
    );
    req.insert(
        "max_price_per_block".into(),
        body.request
            .max_price_per_block
            .map(|p| {
                foundation_serialization::json::Value::Number(
                    foundation_serialization::json::Number::from(p),
                )
            })
            .unwrap_or(foundation_serialization::json::Value::Null),
    );
    req.insert(
        "min_success_rate_ppm".into(),
        body.request
            .min_success_rate_ppm
            .map(|p| {
                foundation_serialization::json::Value::Number(
                    foundation_serialization::json::Number::from(p),
                )
            })
            .unwrap_or(foundation_serialization::json::Value::Null),
    );
    req.insert(
        "limit".into(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.request.limit as u64),
        ),
    );
    map.insert(
        "request".into(),
        foundation_serialization::json::Value::Object(req),
    );
    map.insert(
        "nonce".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.nonce),
        ),
    );
    map.insert(
        "issued_at".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.issued_at),
        ),
    );
    map.insert(
        "ttl".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.ttl),
        ),
    );
    map.insert(
        "origin".to_string(),
        foundation_serialization::json::value_from_slice(&body.origin).unwrap_or(
            foundation_serialization::json::Value::Array(
                body.origin
                    .iter()
                    .map(|b| {
                        foundation_serialization::json::Value::Number(
                            foundation_serialization::json::Number::from(*b as u64),
                        )
                    })
                    .collect(),
            ),
        ),
    );
    foundation_serialization::json::to_vec_value(&foundation_serialization::json::Value::Object(
        map,
    ))
}

fn serialize_lookup_response_body(body: &ProviderLookupResponseBody<'_>) -> Vec<u8> {
    let mut map = foundation_serialization::json::Map::new();
    map.insert(
        "nonce".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.nonce),
        ),
    );
    map.insert(
        "responder".to_string(),
        foundation_serialization::json::value_from_slice(&body.responder).unwrap_or(
            foundation_serialization::json::Value::Array(
                body.responder
                    .iter()
                    .map(|b| {
                        foundation_serialization::json::Value::Number(
                            foundation_serialization::json::Number::from(*b as u64),
                        )
                    })
                    .collect(),
            ),
        ),
    );
    map.insert(
        "providers".to_string(),
        foundation_serialization::json::Value::Array(
            body.providers
                .iter()
                .map(|p| {
                    let bytes = storage_market::codec::serialize_provider_profile(p)
                        .unwrap_or_default();
                    foundation_serialization::json::value_from_slice(&bytes)
                        .unwrap_or(foundation_serialization::json::Value::Null)
                })
                .collect(),
        ),
    );
    foundation_serialization::json::to_vec_value(&foundation_serialization::json::Value::Object(
        map,
    ))
}

impl NetworkProviderDirectory {
    fn broadcast_lookup(&self, request: DiscoveryRequest) {
        let sk = load_net_key();
        let lookup = ProviderLookupRequest::sign(request, 2, &sk);
        if let Ok(msg) =
            crate::net::Message::new(crate::net::Payload::StorageProviderLookup(lookup), &sk)
        {
            self.broadcast(msg);
        }
    }

    fn send_lookup_response(&self, nonce: u64, providers: Vec<ProviderProfile>, addr: SocketAddr) {
        let sk = load_net_key();
        let response = ProviderLookupResponse::sign(nonce, providers, &sk);
        if let Ok(msg) = crate::net::Message::new(
            crate::net::Payload::StorageProviderLookupResponse(response),
            &sk,
        ) {
            let _ = send_msg(addr, &msg);
        }
    }

    fn forward_lookup(&self, request: ProviderLookupRequest) {
        let sk = load_net_key();
        if let Ok(msg) =
            crate::net::Message::new(crate::net::Payload::StorageProviderLookup(request), &sk)
        {
            self.broadcast(msg);
        }
    }
}
