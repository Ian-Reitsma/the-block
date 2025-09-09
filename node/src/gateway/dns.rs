use super::read_receipt;
use crate::simple_db::SimpleDb;
use crate::ERR_DNS_SIG_INVALID;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use hex;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

#[cfg(feature = "telemetry")]
use crate::telemetry::GATEWAY_DNS_LOOKUP_TOTAL;
use trust_dns_resolver::{config::*, Resolver};

static DNS_DB: Lazy<Mutex<SimpleDb>> = Lazy::new(|| {
    let path = std::env::var("TB_DNS_DB_PATH").unwrap_or_else(|_| "dns_db".into());
    Mutex::new(SimpleDb::open(&path))
});

static ALLOW_EXTERNAL: AtomicBool = AtomicBool::new(false);
const VERIFY_TTL: Duration = Duration::from_secs(3600);

type TxtResolver = Box<dyn Fn(&str) -> Vec<String> + Send + Sync>;
static TXT_RESOLVER: Lazy<Mutex<TxtResolver>> =
    Lazy::new(|| Mutex::new(Box::new(default_txt_resolver)));
static VERIFY_CACHE: Lazy<Mutex<HashMap<String, (bool, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn default_txt_resolver(domain: &str) -> Vec<String> {
    Resolver::new(ResolverConfig::default(), ResolverOpts::default())
        .ok()
        .and_then(|r| r.txt_lookup(domain).ok())
        .map(|lookup| {
            lookup
                .iter()
                .flat_map(|r| {
                    r.txt_data()
                        .iter()
                        .filter_map(|d| std::str::from_utf8(d).ok().map(|s| s.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn set_allow_external(val: bool) {
    ALLOW_EXTERNAL.store(val, Ordering::Relaxed);
}

pub fn set_txt_resolver<F>(f: F)
where
    F: Fn(&str) -> Vec<String> + Send + Sync + 'static,
{
    *TXT_RESOLVER.lock().unwrap() = Box::new(f);
}

pub fn clear_verify_cache() {
    VERIFY_CACHE.lock().unwrap().clear();
}

pub enum DnsError {
    SigInvalid,
}

impl DnsError {
    pub fn code(&self) -> i32 {
        -(ERR_DNS_SIG_INVALID as i32)
    }
    pub fn message(&self) -> &'static str {
        "ERR_DNS_SIG_INVALID"
    }
}

pub fn publish_record(params: &Value) -> Result<serde_json::Value, DnsError> {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let txt = params.get("txt").and_then(|v| v.as_str()).unwrap_or("");
    let pk_hex = params.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
    let sig_hex = params.get("sig").and_then(|v| v.as_str()).unwrap_or("");
    let pk_vec = hex::decode(pk_hex).ok().ok_or(DnsError::SigInvalid)?;
    let sig_vec = hex::decode(sig_hex).ok().ok_or(DnsError::SigInvalid)?;
    let pk: [u8; PUBLIC_KEY_LENGTH] = pk_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let sig_bytes: [u8; SIGNATURE_LENGTH] = sig_vec
        .as_slice()
        .try_into()
        .map_err(|_| DnsError::SigInvalid)?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| DnsError::SigInvalid)?;
    let sig = Signature::from_bytes(&sig_bytes);
    let mut msg = Vec::new();
    msg.extend(domain.as_bytes());
    msg.extend(txt.as_bytes());
    vk.verify(&msg, &sig).map_err(|_| DnsError::SigInvalid)?;
    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    db.insert(&format!("dns_records/{}", domain), txt.as_bytes().to_vec());
    db.insert(&format!("dns_keys/{}", domain), pk_hex.as_bytes().to_vec());
    db.insert(
        &format!("dns_reads/{}", domain),
        0u64.to_le_bytes().to_vec(),
    );
    db.insert(&format!("dns_last/{}", domain), 0u64.to_le_bytes().to_vec());
    Ok(serde_json::json!({"status":"ok"}))
}

fn verify_domain(domain: &str, pk_hex: &str) -> bool {
    if domain.ends_with(".block") {
        return true;
    }
    if !ALLOW_EXTERNAL.load(Ordering::Relaxed) {
        return false;
    }
    let now = Instant::now();
    if let Some((ok, ts)) = VERIFY_CACHE.lock().unwrap().get(domain) {
        if now.duration_since(*ts) < VERIFY_TTL {
            return *ok;
        }
    }
    let txts = {
        let resolver = TXT_RESOLVER.lock().unwrap();
        resolver(domain)
    };
    let ok = txts.iter().any(|t| t.contains(pk_hex));
    VERIFY_CACHE
        .lock()
        .unwrap()
        .insert(domain.to_string(), (ok, now));
    #[cfg(feature = "telemetry")]
    {
        let status = if ok { "verified" } else { "rejected" };
        GATEWAY_DNS_LOOKUP_TOTAL.with_label_values(&[status]).inc();
    }
    if !ok {
        warn!(%domain, "gateway dns verification failed");
    }
    ok
}

pub fn gateway_policy(params: &Value) -> serde_json::Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let key = format!("dns_records/{}", domain);
    let mut db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(bytes) = db.get(&key) {
        if let Ok(txt) = String::from_utf8(bytes) {
            let pk = db
                .get(&format!("dns_keys/{}", domain))
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default();
            if verify_domain(domain, &pk) {
                let reads_key = format!("dns_reads/{}", domain);
                let last_key = format!("dns_last/{}", domain);
                let mut reads = db
                    .get(&reads_key)
                    .map(|v| u64::from_le_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
                    .unwrap_or(0);
                reads += 1;
                db.insert(&reads_key, reads.to_le_bytes().to_vec());
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                db.insert(&last_key, ts.to_le_bytes().to_vec());
                let _ = read_receipt::append(domain, "gateway", txt.len() as u64, false, true);
                return serde_json::json!({
                    "record": txt,
                    "reads_total": reads,
                    "last_access_ts": ts,
                });
            }
        }
    }
    serde_json::json!({
        "record": null,
        "reads_total": 0,
        "last_access_ts": 0,
    })
}

pub fn reads_since(params: &Value) -> serde_json::Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let epoch = params.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0);
    let (total, last) = read_receipt::reads_since(epoch, domain);
    serde_json::json!({"reads_total": total, "last_access_ts": last})
}

pub fn dns_lookup(params: &Value) -> serde_json::Value {
    let domain = params.get("domain").and_then(|v| v.as_str()).unwrap_or("");
    let db = DNS_DB.lock().unwrap_or_else(|e| e.into_inner());
    let txt = db
        .get(&format!("dns_records/{}", domain))
        .and_then(|v| String::from_utf8(v).ok());
    let pk = db
        .get(&format!("dns_keys/{}", domain))
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or_default();
    let verified = txt
        .as_ref()
        .map(|_| verify_domain(domain, &pk))
        .unwrap_or(false);
    serde_json::json!({"record": txt, "verified": verified})
}
