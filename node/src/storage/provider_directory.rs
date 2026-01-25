#![forbid(unsafe_code)]

use crate::net::peer::{addr_for_pk, known_peers_with_info, pk_from_addr};
#[cfg(feature = "quic")]
use crate::net::send_quic_msg;
use crate::net::{load_net_key, send_msg, Transport};
use crate::simple_db::{names, SimpleDb};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_PROVIDER_ADVERT_SEEN_TOTAL, STORAGE_PROVIDER_CANDIDATE_GAUGE,
    STORAGE_PROVIDER_DISCOVERY_LATENCY_SECONDS, STORAGE_PROVIDER_PUBLISH_TOTAL,
    STORAGE_PROVIDER_STALE_REJECT_TOTAL,
};
use concurrency::{mutex, Bytes, MutexExt, MutexT};
use crypto_suite::hashing::blake3::hash;
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use rand::RngCore;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::hash::Hash;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use storage_market::{DiscoveryRequest, ProviderDirectory, ProviderProfile, StorageMarket};

type StorageMarketHandle = Arc<MutexT<StorageMarket>>;

const LOOKUP_MAX_AGE_SECS: u64 = 30;
const LOOKUP_RATE_LIMIT_SECS: u64 = 5;
const MAX_PROVIDER_RESULTS: usize = 64;
const MAX_QUERY_FANOUT: usize = 6;
const MAX_QUERY_PATH: usize = 8;
const MAX_SEEN_ENTRIES: usize = 2_048;
const ADVERT_RATE_LIMIT_SECS: u64 = 30;
const MAX_ADVERT_BYTES: usize = 16 * 1024;

pub type StorageProviderQuery = ProviderLookupRequest;
pub type StorageProviderQueryResponse = ProviderLookupResponse;

fn default_ttl() -> u8 {
    2
}

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
    #[serde(default)]
    pub path: Vec<[u8; 32]>,
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Bytes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderLookupResponse {
    pub nonce: u64,
    pub responder: [u8; 32],
    pub providers: Vec<ProviderProfile>,
    #[serde(default)]
    pub path: Vec<[u8; 32]>,
    #[serde(default = "default_ttl")]
    pub ttl: u8,
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
    path: &'a [[u8; 32]],
    ttl: u8,
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
        let ttl = ttl.max(1).min(MAX_QUERY_PATH as u8);
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
            path: vec![body.origin],
            signature: Bytes::from(sig.to_bytes().to_vec()),
        }
    }

    pub fn verify(&self) -> bool {
        if now().saturating_sub(self.issued_at) > LOOKUP_MAX_AGE_SECS {
            return false;
        }
        if self.ttl == 0 || self.ttl > MAX_QUERY_PATH as u8 {
            return false;
        }
        if self.path.len() > MAX_QUERY_PATH {
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
    pub fn sign(
        nonce: u64,
        providers: Vec<ProviderProfile>,
        path: Vec<[u8; 32]>,
        ttl: u8,
        sk: &SigningKey,
    ) -> Self {
        let ttl = ttl.max(1).min(MAX_QUERY_PATH as u8);
        let body = ProviderLookupResponseBody {
            nonce,
            responder: sk.verifying_key().to_bytes(),
            providers: &providers,
            path: &path,
            ttl,
        };
        let bytes = serialize_lookup_response_body(&body);
        let sig = sk.sign(&bytes);
        Self {
            nonce,
            responder: body.responder,
            providers,
            path,
            ttl,
            signature: Bytes::from(sig.to_bytes().to_vec()),
        }
    }

    pub fn verify(&self) -> bool {
        if self.ttl == 0 || self.ttl > MAX_QUERY_PATH as u8 {
            return false;
        }
        if self.path.len() > MAX_QUERY_PATH {
            return false;
        }
        let vk = match VerifyingKey::from_bytes(&self.responder) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let body = ProviderLookupResponseBody {
            nonce: self.nonce,
            responder: self.responder,
            providers: &self.providers,
            path: &self.path,
            ttl: self.ttl,
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
    store: MutexT<SimpleDb>,
    seen_publishers: MutexT<HashMap<[u8; 32], u64>>,
    seen_requests: MutexT<HashMap<(u64, [u8; 32]), u64>>,
    seen_responses: MutexT<HashMap<(u64, [u8; 32]), u64>>,
    last_request_from_origin: MutexT<HashMap<[u8; 32], u64>>,
}

impl NetworkProviderDirectory {
    pub fn new(market: StorageMarketHandle, ttl_secs: u64, store: SimpleDb) -> Self {
        Self {
            market,
            cache: mutex(HashMap::new()),
            ttl_secs,
            store: mutex(store),
            seen_publishers: mutex(HashMap::new()),
            seen_requests: mutex(HashMap::new()),
            seen_responses: mutex(HashMap::new()),
            last_request_from_origin: mutex(HashMap::new()),
        }
    }

    fn load_persisted(&self) {
        let now = now();
        let entries = self
            .store
            .guard()
            .scan_prefix("advert|")
            .unwrap_or_default();
        for (_, bytes) in entries {
            if bytes.len() > MAX_ADVERT_BYTES {
                continue;
            }
            if let Ok(advert) =
                foundation_serialization::json::from_slice::<ProviderAdvertisement>(&bytes)
            {
                if advert.expires_at > now {
                    self.ingest_advertisement(advert);
                }
            }
        }
    }

    pub fn ingest_advertisement(&self, advert: ProviderAdvertisement) {
        let ts = now();
        if advert.expires_at <= ts || !advert.verify() {
            #[cfg(feature = "telemetry")]
            STORAGE_PROVIDER_STALE_REJECT_TOTAL.inc();
            return;
        }
        if advert.ttl_secs > self.ttl_secs.saturating_mul(2) {
            #[cfg(feature = "telemetry")]
            STORAGE_PROVIDER_STALE_REJECT_TOTAL.inc();
            return;
        }
        if let Ok(bytes) = foundation_serialization::json::to_vec(&advert) {
            if bytes.len() > MAX_ADVERT_BYTES {
                #[cfg(feature = "telemetry")]
                STORAGE_PROVIDER_STALE_REJECT_TOTAL.inc();
                return;
            }
        }
        if self.publisher_rate_limited(advert.publisher, ts) {
            return;
        }
        let mut profile = advert.profile.clone();
        profile.mark_version(advert.version);
        profile.set_expiry(advert.expires_at);
        if let Some(updated) = self.cache_profile(profile.clone()) {
            self.track_publisher(advert.publisher, ts);
            let _ = self.market.guard().cache_provider_profile(updated);
            self.persist_advertisement(&advert);
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
        let mut request = request;
        if !request.verify() {
            return;
        }
        let ts = now();
        {
            let mut guard = self.seen_requests.guard();
            prune_seen_map(&mut guard, ts);
            if guard.contains_key(&(request.nonce, request.origin)) {
                return;
            }
            guard.insert((request.nonce, request.origin), ts);
        }
        if request.path.len() > MAX_QUERY_PATH {
            return;
        }
        if self.rate_limited(request.origin, request.issued_at) {
            return;
        }
        let self_pk = load_net_key().verifying_key().to_bytes();
        if request.path.contains(&self_pk) {
            return;
        }
        if request.path.len() < MAX_QUERY_PATH {
            request.path.push(self_pk);
        }
        let ttl_exhausted = request.path.len() > request.ttl as usize;

        let matches = self.discover(&request.request).unwrap_or_default();
        self.send_lookup_response(&request, matches.clone(), responder);

        if !ttl_exhausted {
            self.forward_lookup(request, responder);
        }
    }

    pub fn ingest_lookup_response(&self, response: ProviderLookupResponse) {
        if !response.verify() {
            return;
        }
        {
            let mut guard = self.seen_responses.guard();
            let ts = now();
            prune_seen_map(&mut guard, ts);
            if guard.contains_key(&(response.nonce, response.responder)) {
                return;
            }
            guard.insert((response.nonce, response.responder), ts);
        }
        let forward = response.clone();
        for profile in response.providers.into_iter().take(MAX_PROVIDER_RESULTS) {
            let _ = self.cache_profile(profile.clone());
            let _ = self.market.guard().cache_provider_profile(profile);
        }
        self.forward_response(forward);
    }

    fn rate_limited(&self, origin: [u8; 32], issued_at: u64) -> bool {
        let mut guard = self.last_request_from_origin.guard();
        prune_seen_map(&mut guard, issued_at);
        let last = guard.get(&origin).copied().unwrap_or(0);
        if issued_at.saturating_sub(last) < LOOKUP_RATE_LIMIT_SECS {
            return true;
        }
        guard.insert(origin, issued_at);
        false
    }

    fn publisher_rate_limited(&self, publisher: [u8; 32], ts: u64) -> bool {
        let mut guard = self.seen_publishers.guard();
        prune_seen_map(&mut guard, ts);
        if let Some(last) = guard.get(&publisher) {
            if ts.saturating_sub(*last) < ADVERT_RATE_LIMIT_SECS {
                return true;
            }
        }
        if guard.len() > MAX_SEEN_ENTRIES {
            let mut entries: Vec<_> = guard.iter().map(|(k, t)| (*k, *t)).collect();
            entries.sort_by_key(|(_, t)| *t);
            let surplus = guard.len().saturating_sub(MAX_SEEN_ENTRIES);
            for (key, _) in entries.into_iter().take(surplus) {
                guard.remove(&key);
            }
        }
        false
    }

    fn track_publisher(&self, publisher: [u8; 32], ts: u64) {
        let mut guard = self.seen_publishers.guard();
        guard.insert(publisher, ts);
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

    fn persist_advertisement(&self, advert: &ProviderAdvertisement) {
        if let Ok(bytes) = foundation_serialization::json::to_vec(advert) {
            if bytes.len() > MAX_ADVERT_BYTES {
                return;
            }
            let key = format!("advert|{}", advert.profile.provider_id);
            let _ = self.store.guard().insert(&key, bytes);
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
        out.sort_by(|a, b| {
            a.price_per_block
                .cmp(&b.price_per_block)
                .then_with(|| b.last_seen_block.cmp(&a.last_seen_block))
                .then_with(|| b.version.cmp(&a.version))
                .then_with(|| a.provider_id.cmp(&b.provider_id))
        });
        if out.len() > MAX_PROVIDER_RESULTS {
            out.truncate(MAX_PROVIDER_RESULTS);
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

fn provider_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("TB_STORAGE_PROVIDER_DB") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("TB_STORAGE_MARKET_DIR") {
        return PathBuf::from(path).join("provider_directory");
    }
    PathBuf::from("storage_provider_directory")
}

fn prune_seen_map<K>(map: &mut HashMap<K, u64>, now: u64)
where
    K: Eq + Hash + Clone,
{
    map.retain(|_, ts| now.saturating_sub(*ts) <= LOOKUP_MAX_AGE_SECS);
    if map.len() > MAX_SEEN_ENTRIES {
        let mut entries: Vec<_> = map.iter().map(|(k, ts)| (k.clone(), *ts)).collect();
        entries.sort_by_key(|(_, ts)| *ts);
        let surplus = map.len().saturating_sub(MAX_SEEN_ENTRIES);
        for (key, _) in entries.into_iter().take(surplus) {
            map.remove(&key);
        }
    }
}

fn routing_key(request: &DiscoveryRequest, origin: [u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&origin);
    buf.extend_from_slice(&request.object_size.to_le_bytes());
    buf.extend_from_slice(&request.shares.to_le_bytes());
    if let Some(region) = &request.region {
        buf.extend_from_slice(region.as_bytes());
    }
    if let Some(price) = request.max_price_per_block {
        buf.extend_from_slice(&price.to_le_bytes());
    }
    let digest = hash(&buf);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_bytes());
    out
}

fn peer_distance(addr: &SocketAddr, key: [u8; 32]) -> u128 {
    if let Some(pk) = pk_from_addr(addr) {
        let mut dist = [0u8; 16];
        for i in 0..16 {
            dist[i] = pk[i] ^ key[i];
        }
        return u128::from_be_bytes(dist);
    }
    let hashed = hash(addr.to_string().as_bytes());
    let mut dist = [0u8; 16];
    dist.copy_from_slice(&hashed.as_bytes()[..16]);
    u128::from_le_bytes(dist)
}

fn previous_hop(path: &[[u8; 32]], self_pk: &[u8; 32]) -> Option<[u8; 32]> {
    path.iter()
        .position(|pk| pk == self_pk)
        .and_then(|idx| idx.checked_sub(1))
        .and_then(|idx| path.get(idx).copied())
}

pub fn install_directory(market: StorageMarketHandle) {
    let store_path = provider_store_path();
    let store = SimpleDb::open_named(
        names::STORAGE_PROVIDER_DIRECTORY,
        &store_path.to_string_lossy(),
    );
    let dir = Arc::new(NetworkProviderDirectory::new(
        market.clone(),
        15 * 60,
        store,
    ));
    if let Ok(profiles) = market.guard().provider_profiles() {
        for profile in profiles {
            let _ = dir.cache_profile(profile.clone());
        }
    }
    dir.load_persisted();
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
                    let bytes =
                        storage_market::codec::serialize_provider_profile(p).unwrap_or_default();
                    foundation_serialization::json::value_from_slice(&bytes)
                        .unwrap_or(foundation_serialization::json::Value::Null)
                })
                .collect(),
        ),
    );
    map.insert(
        "path".to_string(),
        foundation_serialization::json::Value::Array(
            body.path
                .iter()
                .map(|hop| {
                    foundation_serialization::json::value_from_slice(hop).unwrap_or(
                        foundation_serialization::json::Value::Array(
                            hop.iter()
                                .map(|b| {
                                    foundation_serialization::json::Value::Number(
                                        foundation_serialization::json::Number::from(*b as u64),
                                    )
                                })
                                .collect(),
                        ),
                    )
                })
                .collect(),
        ),
    );
    map.insert(
        "ttl".to_string(),
        foundation_serialization::json::Value::Number(
            foundation_serialization::json::Number::from(body.ttl),
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
        self.forward_lookup(lookup, None);
    }

    fn send_lookup_response(
        &self,
        request: &ProviderLookupRequest,
        providers: Vec<ProviderProfile>,
        responder: Option<SocketAddr>,
    ) {
        let sk = load_net_key();
        let mut providers = providers;
        if providers.len() > MAX_PROVIDER_RESULTS {
            providers.truncate(MAX_PROVIDER_RESULTS);
        }
        let response = ProviderLookupResponse::sign(
            request.nonce,
            providers,
            request.path.clone(),
            request.ttl,
            &sk,
        );
        if let Some(addr) = responder {
            if let Ok(msg) = crate::net::Message::new(
                crate::net::Payload::StorageProviderQueryResponse(response.clone()),
                &sk,
            ) {
                let _ = send_msg(addr, &msg);
                return;
            }
        }
        let self_pk = sk.verifying_key().to_bytes();
        if let Some(prev) = previous_hop(&response.path, &self_pk).and_then(|pk| addr_for_pk(&pk)) {
            if let Ok(msg) = crate::net::Message::new(
                crate::net::Payload::StorageProviderQueryResponse(response),
                &sk,
            ) {
                let _ = send_msg(prev, &msg);
            }
        }
    }

    fn forward_lookup(&self, request: ProviderLookupRequest, responder: Option<SocketAddr>) {
        let key = routing_key(&request.request, request.origin);
        let mut peers = self.fanout_peers(key, responder);
        peers.retain(|(addr, _, _)| {
            if let Some(pk) = pk_from_addr(addr) {
                return !request.path.iter().any(|hop| hop == &pk);
            }
            true
        });
        for (addr, transport, cert) in peers {
            let sk = load_net_key();
            if let Ok(msg) = crate::net::Message::new(
                crate::net::Payload::StorageProviderQuery(request.clone()),
                &sk,
            ) {
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

    fn forward_response(&self, response: ProviderLookupResponse) {
        let sk = load_net_key();
        let self_pk = sk.verifying_key().to_bytes();
        if let Some(prev) = previous_hop(&response.path, &self_pk).and_then(|pk| addr_for_pk(&pk)) {
            if let Ok(msg) = crate::net::Message::new(
                crate::net::Payload::StorageProviderQueryResponse(response),
                &sk,
            ) {
                let _ = send_msg(prev, &msg);
            }
        }
    }

    fn fanout_peers(
        &self,
        key: [u8; 32],
        exclude: Option<SocketAddr>,
    ) -> Vec<(SocketAddr, Transport, Option<Bytes>)> {
        let mut peers = known_peers_with_info();
        if let Some(addr) = exclude {
            peers.retain(|(a, _, _)| *a != addr);
        }
        peers.sort_by_key(|(addr, _, _)| peer_distance(addr, key));
        if peers.len() > MAX_QUERY_FANOUT {
            peers.truncate(MAX_QUERY_FANOUT);
        }
        peers
    }
}
