#![cfg(feature = "quic")]
use std::collections::VecDeque;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use ::time::{Duration, OffsetDateTime};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use dirs;
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use hex;
use once_cell::sync::Lazy;
use rand_core::{OsRng, RngCore};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use s2n_quic::{client::Connect, provider::tls, Client, Server};
use serde::{Deserialize, Serialize};
use x509_parser::prelude::*;
use x509_parser::time::ASN1Time;

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    QUIC_CERT_ROTATION_TOTAL, QUIC_HANDSHAKE_FAIL_TOTAL, QUIC_RETRANSMIT_TOTAL,
};

const MAX_PREVIOUS_CERTS: usize = 4;
const CERT_STORE_FILE: &str = "quic_certs.json";
const ED25519_OID: &str = "1.3.101.112";

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

/// Ensure the certificate state is initialised by rotating a fresh
/// certificate and persisting the previous one (if any).
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

/// Rotate the local certificate immediately using the provided signing key.
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

/// Return the currently active certificate, if initialised.
pub fn current_cert() -> Option<LocalCert> {
    STATE.read().unwrap().current.clone()
}

/// Return the certificate advertisement shared with peers.
pub fn current_advertisement() -> Option<CertAdvertisement> {
    let guard = STATE.read().unwrap();
    advertisement_from_state(&guard)
}

/// Return all known fingerprints (current + previous) for local migrations.
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

/// Compute the deterministic fingerprint for a DER encoded certificate.
pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    let hash = blake3::hash(cert);
    let mut fp = [0u8; 32];
    fp.copy_from_slice(hash.as_bytes());
    fp
}

/// Verify that `cert_der` encodes an Ed25519 certificate bound to `peer_key`.
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
    #[cfg(feature = "telemetry")]
    QUIC_CERT_ROTATION_TOTAL.with_label_values(&["local"]).inc();
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

/// Start a QUIC server bound to `addr` using the current certificate derived from the
/// Ed25519 network identity.
pub async fn start_server(addr: SocketAddr) -> Result<Server, Box<dyn std::error::Error>> {
    let key = super::load_net_key();
    let _ = initialize(&key)?;
    let current = current_cert().ok_or_else(|| anyhow!("missing current certificate"))?;
    let server = Server::builder()
        .with_tls((current.cert.as_slice(), current.key.as_slice()))?
        .with_io(addr)?
        .start()?;
    Ok(server)
}

/// Establish a QUIC connection to `addr` using default TLS.
pub async fn connect(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .with_tls(tls::Default::default())?
        .with_io("0.0.0.0:0")?
        .start()?;
    let _connection = client
        .connect(Connect::new(addr).with_server_name("the-block"))
        .await?;
    Ok(())
}

/// Record a handshake failure for telemetry.
pub fn record_handshake_fail(reason: &str) {
    #[cfg(feature = "telemetry")]
    {
        let peer_label = super::quic_stats::peer_label(None);
        QUIC_HANDSHAKE_FAIL_TOTAL
            .with_label_values(&[peer_label.as_str(), reason])
            .inc();
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = reason;
}

/// Record a retransmission event for telemetry.
pub fn record_retransmit(count: u64) {
    #[cfg(feature = "telemetry")]
    QUIC_RETRANSMIT_TOTAL.inc_by(count);
    #[cfg(not(feature = "telemetry"))]
    let _ = count;
}
