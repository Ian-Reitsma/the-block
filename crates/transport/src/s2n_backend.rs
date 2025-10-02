use crypto_suite::hashing::blake3;
use std::collections::VecDeque;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use ::time::{Duration, OffsetDateTime};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use crypto_suite::signatures::ed25519::{KeyEncodingError, SigningKey};
use dirs;
use once_cell::sync::Lazy;
use rand_core::{OsRng, RngCore};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
pub use s2n_quic::{client::Connect, provider::tls, Client, Server};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use x509_parser::prelude::*;
use x509_parser::time::ASN1Time;

use crate::ProviderCapability;

const MAX_PREVIOUS_CERTS: usize = 4;
const CERT_STORE_FILE: &str = "quic_certs.json";
const ED25519_OID: &str = "1.3.101.112";

#[derive(Clone, Default)]
pub struct S2nEventCallbacks {
    pub cert_rotated: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
    pub handshake_failure: Option<Arc<dyn Fn(&str) + Send + Sync + 'static>>,
    pub retransmit: Option<Arc<dyn Fn(u64) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

#[derive(Debug)]
pub enum S2nCallbackError {
    AlreadyInstalled,
}

static CALLBACKS: OnceLock<RwLock<Arc<S2nEventCallbacks>>> = OnceLock::new();
static HANDSHAKE_TIMEOUT: OnceLock<RwLock<StdDuration>> = OnceLock::new();

fn with_callbacks<F>(f: F)
where
    F: FnOnce(&S2nEventCallbacks),
{
    if let Some(lock) = CALLBACKS.get() {
        let callbacks = lock.read().unwrap().clone();
        f(callbacks.as_ref());
    }
}

pub fn set_event_callbacks(callbacks: S2nEventCallbacks) -> Result<(), S2nCallbackError> {
    let lock = CALLBACKS.get_or_init(|| RwLock::new(Arc::new(S2nEventCallbacks::default())));
    let mut guard = lock.write().unwrap();
    *guard = Arc::new(callbacks);
    Ok(())
}

fn handshake_timeout() -> StdDuration {
    HANDSHAKE_TIMEOUT
        .get_or_init(|| RwLock::new(StdDuration::from_secs(5)))
        .read()
        .unwrap()
        .clone()
}

pub fn set_handshake_timeout(timeout: StdDuration) {
    let lock = HANDSHAKE_TIMEOUT.get_or_init(|| RwLock::new(StdDuration::from_secs(5)));
    *lock.write().unwrap() = timeout;
}

#[derive(Clone, Debug)]
pub struct LocalCert {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    pub fingerprint: [u8; 32],
    pub issued_at: u64,
}

#[derive(Clone, Debug)]
pub struct CertAdvertisement {
    pub cert: Vec<u8>,
    pub fingerprint: [u8; 32],
    pub previous: Vec<[u8; 32]>,
}

#[derive(Default, Clone)]
struct CertState {
    current: Option<LocalCert>,
    previous: VecDeque<HistoricalCert>,
}

#[derive(Clone, Debug)]
struct HistoricalCert {
    cert: Vec<u8>,
    fingerprint: [u8; 32],
    issued_at: u64,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct StoredState {
    current: Option<StoredCert>,
    previous: Vec<StoredCert>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredCert {
    cert: String,
    fingerprint: String,
    issued_at: u64,
}

static STATE: Lazy<RwLock<CertState>> = Lazy::new(|| RwLock::new(CertState::default()));
static LOADED: OnceLock<()> = OnceLock::new();
static CERT_STORE_OVERRIDE: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

pub fn set_cert_store_path(path: Option<PathBuf>) {
    *CERT_STORE_OVERRIDE.write().unwrap() = path;
}

pub const CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::CertificateRotation,
    ProviderCapability::TelemetryCallbacks,
];

pub const PROVIDER_ID: &str = "s2n-quic";

pub fn initialize(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    {
        let guard = STATE.read().unwrap();
        if let Some(advert) = advertisement_from_state(&guard) {
            return Ok(advert);
        }
    }
    let mut guard = STATE.write().unwrap();
    load_from_disk(&mut guard)?;
    rotate_state(signing_key, &mut guard)?;
    persist_state(&guard)?;
    advertisement_from_state(&guard)
        .ok_or_else(|| anyhow!("certificate state missing after initialization"))
}

pub fn rotate(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    let mut guard = STATE.write().unwrap();
    if guard.current.is_none() {
        load_from_disk(&mut guard)?;
    }
    rotate_state(signing_key, &mut guard)?;
    persist_state(&guard)?;
    advertisement_from_state(&guard)
        .ok_or_else(|| anyhow!("certificate state missing after rotation"))
}

pub fn current_cert() -> Option<LocalCert> {
    STATE.read().unwrap().current.clone()
}

pub fn current_advertisement() -> Option<CertAdvertisement> {
    let guard = STATE.read().unwrap();
    advertisement_from_state(&guard)
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    let guard = STATE.read().unwrap();
    let mut entries = Vec::with_capacity(guard.previous.len() + 1);
    if let Some(curr) = guard.current.as_ref() {
        entries.push(curr.fingerprint);
    }
    for prev in guard.previous.iter() {
        entries.push(prev.fingerprint);
    }
    entries
}

pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    let hash = blake3::hash(cert);
    let mut fp = [0u8; 32];
    fp.copy_from_slice(hash.as_bytes());
    fp
}

pub fn verify_remote_certificate(peer_key: &[u8; 32], cert_der: &[u8]) -> Result<[u8; 32]> {
    let (_, parsed) =
        X509Certificate::from_der(cert_der).map_err(|_| anyhow!("invalid x509 certificate"))?;
    if parsed.subject_pki.algorithm.algorithm.to_id_string() != ED25519_OID {
        return Err(anyhow!("unexpected certificate algorithm"));
    }
    let pk_bytes = parsed.subject_pki.subject_public_key.data.as_ref();
    if pk_bytes != peer_key {
        return Err(anyhow!("certificate public key mismatch"));
    }
    let now = ASN1Time::now();
    if parsed.validity().not_before > now || parsed.validity().not_after < now {
        return Err(anyhow!("certificate expired"));
    }
    Ok(fingerprint(cert_der))
}

fn advertisement_from_state(state: &CertState) -> Option<CertAdvertisement> {
    state.current.as_ref().map(|curr| CertAdvertisement {
        cert: curr.cert.clone(),
        fingerprint: curr.fingerprint,
        previous: state.previous.iter().map(|h| h.fingerprint).collect(),
    })
}

fn rotate_state(signing_key: &SigningKey, state: &mut CertState) -> Result<()> {
    if let Some(existing) = state.current.take() {
        state.previous.push_front(HistoricalCert {
            cert: existing.cert,
            fingerprint: existing.fingerprint,
            issued_at: existing.issued_at,
        });
    }
    let fresh = generate_local_cert(signing_key)?;
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.cert_rotated.as_ref() {
            cb("local");
        }
    });
    state.current = Some(fresh);
    while state.previous.len() > MAX_PREVIOUS_CERTS {
        state.previous.pop_back();
    }
    Ok(())
}

fn generate_local_cert(signing_key: &SigningKey) -> Result<LocalCert> {
    let issued_at = now_secs();
    let mut params = CertificateParams::default();
    params.alg = &rcgen::PKCS_ED25519;
    params.distinguished_name = {
        let mut dn = DistinguishedName::new();
        let hex_id = hex::encode(signing_key.verifying_key().to_bytes());
        dn.push(DnType::CommonName, format!("the-block node {hex_id}"));
        dn
    };
    params.subject_alt_names = vec![SanType::DnsName("the-block.local".into())];
    params.not_before = OffsetDateTime::now_utc() - Duration::hours(1);
    params.not_after = OffsetDateTime::now_utc() + Duration::days(7);
    params.serial_number = Some(random_serial());
    let key_der = signing_key
        .to_pkcs8_der()
        .map_err(|e| anyhow!("pkcs8 encoding failed: {e}"))?;
    let key_pair = KeyPair::from_der(key_der.as_bytes())?;
    params.key_pair = Some(key_pair);
    let cert = Certificate::from_params(params)?;
    let cert_der = cert.serialize_der()?;
    let mut fp = [0u8; 32];
    fp.copy_from_slice(blake3::hash(&cert_der).as_bytes());
    Ok(LocalCert {
        cert: cert_der,
        key: key_der.as_bytes().to_vec(),
        fingerprint: fp,
        issued_at,
    })
}

fn random_serial() -> rcgen::SerialNumber {
    let mut buf = [0u8; 16];
    OsRng.fill_bytes(&mut buf);
    buf[0] &= 0x7F;
    rcgen::SerialNumber::from_slice(&buf)
}

fn load_from_disk(state: &mut CertState) -> Result<()> {
    if LOADED.get().is_some() {
        return Ok(());
    }
    let path = cert_store_path();
    let data = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            LOADED.set(()).ok();
            return Ok(());
        }
        Err(e) => return Err(anyhow!("read cert store failed: {e}")),
    };
    let stored: StoredState =
        serde_json::from_slice(&data).map_err(|e| anyhow!("decode cert store failed: {e}"))?;
    let mut queue = VecDeque::new();
    if let Some(curr) = stored.current {
        if let Ok(hist) = stored_to_hist(curr) {
            queue.push_back(hist);
        }
    }
    for prev in stored.previous {
        if let Ok(hist) = stored_to_hist(prev) {
            queue.push_back(hist);
        }
    }
    state.previous = queue;
    LOADED.set(()).ok();
    Ok(())
}

fn persist_state(state: &CertState) -> Result<()> {
    let path = cert_store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create cert store dir")?;
    }
    let stored = StoredState {
        current: state.current.as_ref().map(local_to_stored),
        previous: state.previous.iter().map(hist_to_stored).collect(),
    };
    let data = serde_json::to_vec_pretty(&stored)?;
    fs::write(path, data).context("write cert store")?;
    Ok(())
}

fn stored_to_hist(stored: StoredCert) -> Result<HistoricalCert> {
    let cert = B64
        .decode(stored.cert.as_bytes())
        .map_err(|e| anyhow!("invalid stored cert: {e}"))?;
    let bytes =
        hex::decode(stored.fingerprint).map_err(|e| anyhow!("invalid stored fingerprint: {e}"))?;
    if bytes.len() != 32 {
        return Err(anyhow!("invalid fingerprint length"));
    }
    let mut fp = [0u8; 32];
    fp.copy_from_slice(&bytes);
    Ok(HistoricalCert {
        cert,
        fingerprint: fp,
        issued_at: stored.issued_at,
    })
}

fn hist_to_stored(hist: &HistoricalCert) -> StoredCert {
    StoredCert {
        cert: B64.encode(&hist.cert),
        fingerprint: hex::encode(hist.fingerprint),
        issued_at: hist.issued_at,
    }
}

fn local_to_stored(local: &LocalCert) -> StoredCert {
    StoredCert {
        cert: B64.encode(&local.cert),
        fingerprint: hex::encode(local.fingerprint),
        issued_at: local.issued_at,
    }
}

fn cert_store_path() -> PathBuf {
    if let Some(path) = CERT_STORE_OVERRIDE.read().unwrap().as_ref() {
        return path.clone();
    }
    if let Ok(path) = std::env::var("TB_NET_CERT_STORE_PATH") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".the_block")
        .join(CERT_STORE_FILE)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub async fn start_server(
    addr: SocketAddr,
    signing_key: &SigningKey,
) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
    let _ = initialize(signing_key)?;
    let current = current_cert().ok_or_else(|| anyhow!("missing current certificate"))?;
    let server = Server::builder()
        .with_tls((current.cert.as_slice(), current.key.as_slice()))?
        .with_io(addr)?
        .start()?;
    Ok(Arc::new(server))
}

pub async fn connect(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .with_tls(tls::Default::default())?
        .with_io("0.0.0.0:0")?
        .start()?;
    let fut = client.connect(Connect::new(addr).with_server_name("the-block"));
    match runtime::timeout(handshake_timeout(), fut).await {
        Ok(Ok(_connection)) => {
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.provider_connect.as_ref() {
                    cb(PROVIDER_ID);
                }
            });
            Ok(())
        }
        Ok(Err(err)) => Err(Box::new(err)),
        Err(_) => Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "s2n handshake timed out",
        ))),
    }
}

pub fn record_handshake_fail(reason: &str) {
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.handshake_failure.as_ref() {
            cb(reason);
        }
    });
}

pub fn record_retransmit(count: u64) {
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.retransmit.as_ref() {
            cb(count);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tempfile::TempDir;

    fn reset_state() {
        *STATE.write().unwrap() = CertState::default();
    }

    #[test]
    fn verify_remote_certificate_matches_generated_advertisement() {
        let temp = TempDir::new().expect("temp dir");
        set_cert_store_path(Some(temp.path().join("quic-test.json")));
        reset_state();

        let mut secret = [0u8; 32];
        OsRng.fill_bytes(&mut secret);
        let signing = SigningKey::from_bytes(&secret);
        let advert = initialize(&signing).expect("initialize transport certs");
        let mut peer_key = signing.verifying_key().to_bytes();

        let fingerprint =
            verify_remote_certificate(&peer_key, &advert.cert).expect("valid certificate");
        assert_eq!(fingerprint, advert.fingerprint);

        peer_key[0] ^= 0xFF;
        assert!(verify_remote_certificate(&peer_key, &advert.cert).is_err());

        set_cert_store_path(None);
        reset_state();
    }

    #[test]
    fn replacing_callbacks_updates_handlers() {
        let counter = Arc::new(AtomicUsize::new(0));
        let first = counter.clone();
        set_event_callbacks({
            let mut callbacks = S2nEventCallbacks::default();
            callbacks.handshake_failure = Some(Arc::new(move |_reason| {
                first.fetch_add(1, Ordering::SeqCst);
            }));
            callbacks
        })
        .expect("install callbacks");

        record_handshake_fail("first");
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        let second = counter.clone();
        set_event_callbacks({
            let mut callbacks = S2nEventCallbacks::default();
            callbacks.handshake_failure = Some(Arc::new(move |_reason| {
                second.fetch_add(5, Ordering::SeqCst);
            }));
            callbacks
        })
        .expect("replace callbacks");

        record_handshake_fail("second");
        assert_eq!(counter.load(Ordering::SeqCst), 6);

        let _ = set_event_callbacks(S2nEventCallbacks::default());
    }
}
