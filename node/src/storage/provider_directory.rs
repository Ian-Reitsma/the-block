#![forbid(unsafe_code)]

use crate::net::{known_peers_with_info, load_net_key, send_msg};
#[cfg(feature = "quic")]
use crate::net::send_quic_msg;
use crate::rpc::storage::StorageMarketHandle;
use concurrency::{mutex, MutexExt, MutexT};
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use storage_market::{DiscoveryRequest, ProviderDirectory, ProviderProfile};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    STORAGE_PROVIDER_ADVERT_SEEN_TOTAL, STORAGE_PROVIDER_CANDIDATE_GAUGE,
    STORAGE_PROVIDER_DISCOVERY_LATENCY_SECONDS, STORAGE_PROVIDER_PUBLISH_TOTAL,
    STORAGE_PROVIDER_STALE_REJECT_TOTAL,
};
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderAdvertisement {
    pub profile: ProviderProfile,
    pub version: u64,
    pub ttl_secs: u64,
    pub expires_at: u64,
    pub publisher: [u8; 32],
    pub signature: Vec<u8>,
}

#[derive(Serialize)]
struct ProviderAdvertisementBody {
    profile: ProviderProfile,
    version: u64,
    ttl_secs: u64,
    expires_at: u64,
    publisher: [u8; 32],
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
            signature: sig.to_bytes().to_vec(),
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
        let sig = match Signature::from_bytes(&self.signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        vk.verify(&bytes, &sig).is_ok()
    }
}

pub struct NetworkProviderDirectory {
    market: StorageMarketHandle,
    cache: MutexT<HashMap<String, ProviderProfile>>,
    ttl_secs: u64,
    seen_publishers: MutexT<HashSet<[u8; 32]>>,
}

impl NetworkProviderDirectory {
    pub fn new(market: StorageMarketHandle, ttl_secs: u64) -> Self {
        Self {
            market,
            cache: mutex(HashMap::new()),
            ttl_secs,
            seen_publishers: mutex(HashSet::new()),
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
        let payload =
            crate::net::Message::new(crate::net::Payload::StorageProviderAdvertisement(advert), &sk);
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
                crate::net::Transport::Tcp => {
                    let _ = send_msg(addr, &msg);
                }
                crate::net::Transport::Quic => {
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
        Ok(out)
    }
}

type DirectoryHandle = Arc<MutexT<Option<Arc<NetworkProviderDirectory>>>>;

static DIRECTORY: concurrency::Lazy<DirectoryHandle> = concurrency::Lazy::new(|| mutex(None));

pub fn install_directory(market: StorageMarketHandle) {
    let dir = Arc::new(NetworkProviderDirectory::new(market, 15 * 60));
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
