use crypto_suite::hashing::blake3;
use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use ::time::{Duration, OffsetDateTime};
use base64_fp::{decode_standard, encode_standard};
use crypto_suite::signatures::ed25519::{SigningKey, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH};
use diagnostics::{anyhow, Context, Result};
use rand::{OsRng, RngCore};
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, KeyPair, RemoteKeyPair, SanType,
};
use runtime::net::UdpSocket;
use runtime::sync::Mutex as AsyncMutex;
use runtime::{sleep, timeout, TimeoutError};
use serde::{Deserialize, Serialize};
use x509_parser::prelude::*;
use x509_parser::time::ASN1Time;

use crate::ProviderCapability;

const MAX_PREVIOUS_CERTS: usize = 4;
const CERT_STORE_FILE: &str = "quic_certs.json";
const ED25519_OID: &str = "1.3.101.112";
const HANDSHAKE_MAGIC: [u8; 4] = *b"TBHS";
const HANDSHAKE_ACK_MAGIC: [u8; 4] = *b"TBHA";
const HANDSHAKE_PACKET_LEN: usize = 12;
const MAX_HANDSHAKE_ATTEMPTS: usize = 3;
const HANDSHAKE_BACKOFF: StdDuration = StdDuration::from_millis(50);

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

pub fn set_event_callbacks(
    callbacks: S2nEventCallbacks,
) -> std::result::Result<(), S2nCallbackError> {
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

struct SigningRemoteKey {
    secret: [u8; SECRET_KEY_LENGTH],
    public: [u8; PUBLIC_KEY_LENGTH],
}

impl SigningRemoteKey {
    fn new(key: &SigningKey) -> Self {
        Self {
            secret: key.to_bytes(),
            public: key.verifying_key().to_bytes(),
        }
    }
}

impl RemoteKeyPair for SigningRemoteKey {
    fn public_key(&self) -> &[u8] {
        &self.public
    }

    fn sign(&self, msg: &[u8]) -> std::result::Result<Vec<u8>, rcgen::Error> {
        let signer = SigningKey::from_bytes(&self.secret);
        let signature = signer.sign(msg);
        Ok(signature.to_bytes().to_vec())
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ED25519
    }
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

static STATE: OnceLock<RwLock<CertState>> = OnceLock::new();
static LOADED: OnceLock<()> = OnceLock::new();
static CERT_STORE_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn cert_state() -> &'static RwLock<CertState> {
    STATE.get_or_init(|| RwLock::new(CertState::default()))
}

fn cert_store_override() -> &'static RwLock<Option<PathBuf>> {
    CERT_STORE_OVERRIDE.get_or_init(|| RwLock::new(None))
}

pub fn set_cert_store_path(path: Option<PathBuf>) {
    *cert_store_override().write().unwrap() = path;
}

pub const CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::CertificateRotation,
    ProviderCapability::TelemetryCallbacks,
];

pub const PROVIDER_ID: &str = "s2n-quic";

pub fn initialize(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    {
        let guard = cert_state().read().unwrap();
        if let Some(advert) = advertisement_from_state(&guard) {
            return Ok(advert);
        }
    }
    let mut guard = cert_state().write().unwrap();
    load_from_disk(&mut guard)?;
    rotate_state(signing_key, &mut guard)?;
    persist_state(&guard)?;
    advertisement_from_state(&guard)
        .ok_or_else(|| anyhow!("certificate state missing after initialization"))
}

pub fn rotate(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    let mut guard = cert_state().write().unwrap();
    if guard.current.is_none() {
        load_from_disk(&mut guard)?;
    }
    rotate_state(signing_key, &mut guard)?;
    persist_state(&guard)?;
    advertisement_from_state(&guard)
        .ok_or_else(|| anyhow!("certificate state missing after rotation"))
}

pub fn current_cert() -> Option<LocalCert> {
    cert_state().read().unwrap().current.clone()
}

pub fn current_advertisement() -> Option<CertAdvertisement> {
    let guard = cert_state().read().unwrap();
    advertisement_from_state(&guard)
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    let guard = cert_state().read().unwrap();
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
    let remote = SigningRemoteKey::new(signing_key);
    let key_pair = KeyPair::from_remote(Box::new(remote)).map_err(|err| anyhow!(err))?;
    params.key_pair = Some(key_pair);
    let cert = Certificate::from_params(params).map_err(|err| anyhow!(err))?;
    let cert_der = cert.serialize_der().map_err(|err| anyhow!(err))?;
    let mut fp = [0u8; 32];
    fp.copy_from_slice(blake3::hash(&cert_der).as_bytes());
    Ok(LocalCert {
        cert: cert_der,
        key: signing_key.to_keypair_bytes().to_vec(),
        fingerprint: fp,
        issued_at,
    })
}

fn random_serial() -> rcgen::SerialNumber {
    let mut buf = [0u8; 16];
    OsRng::default().fill_bytes(&mut buf);
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
    let data = serde_json::to_vec_pretty(&stored).map_err(|err| anyhow!(err))?;
    fs::write(path, data).context("write cert store")?;
    Ok(())
}

fn stored_to_hist(stored: StoredCert) -> Result<HistoricalCert> {
    let cert = decode_standard(&stored.cert).map_err(|e| anyhow!("invalid stored cert: {e}"))?;
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
        cert: encode_standard(&hist.cert),
        fingerprint: hex::encode(hist.fingerprint),
        issued_at: hist.issued_at,
    }
}

fn local_to_stored(local: &LocalCert) -> StoredCert {
    StoredCert {
        cert: encode_standard(&local.cert),
        fingerprint: hex::encode(local.fingerprint),
        issued_at: local.issued_at,
    }
}

fn cert_store_path() -> PathBuf {
    if let Some(path) = cert_store_override().read().unwrap().as_ref() {
        return path.clone();
    }
    if let Ok(path) = std::env::var("TB_NET_CERT_STORE_PATH") {
        return PathBuf::from(path);
    }
    sys::paths::home_dir()
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

pub async fn start_server(addr: SocketAddr, signing_key: &SigningKey) -> Result<Arc<Server>> {
    let _ = initialize(signing_key)?;
    let socket = UdpSocket::bind(addr).await.map_err(|err| anyhow!(err))?;
    let local_addr = socket.local_addr().map_err(|err| anyhow!(err))?;
    Ok(Arc::new(Server::new(socket, local_addr)))
}

pub async fn connect(addr: SocketAddr) -> Result<()> {
    let mut socket = UdpSocket::bind("0.0.0.0:0".parse().map_err(|err| anyhow!(err))?)
        .await
        .map_err(|err| anyhow!(err))?;
    let mut token_bytes = [0u8; 8];
    OsRng::default().fill_bytes(&mut token_bytes);
    let token = u64::from_be_bytes(token_bytes);

    let mut packet = [0u8; HANDSHAKE_PACKET_LEN];
    packet[..4].copy_from_slice(&HANDSHAKE_MAGIC);
    packet[4..12].copy_from_slice(&token_bytes);

    let mut attempt = 0usize;
    loop {
        socket
            .send_to(&packet, addr)
            .await
            .map_err(|err| anyhow!(err))?;
        let wait = wait_for_ack(&mut socket, addr, token);
        match timeout(handshake_timeout(), wait).await {
            Ok(Ok(())) => {
                with_callbacks(|cbs| {
                    if let Some(cb) = cbs.provider_connect.as_ref() {
                        cb(PROVIDER_ID);
                    }
                });
                return Ok(());
            }
            Ok(Err(err)) => {
                record_handshake_fail("io_error");
                return Err(anyhow!(err));
            }
            Err(TimeoutError { .. }) => {
                attempt += 1;
                if attempt >= MAX_HANDSHAKE_ATTEMPTS {
                    record_handshake_fail("timeout");
                    return Err(anyhow!(io::Error::new(
                        ErrorKind::TimedOut,
                        "s2n handshake timed out",
                    )));
                }
                record_retransmit(attempt as u64);
                sleep(HANDSHAKE_BACKOFF).await;
            }
        }
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

pub struct Server {
    socket: Arc<AsyncMutex<UdpSocket>>,
    local_addr: SocketAddr,
}

impl Server {
    fn new(socket: UdpSocket, local_addr: SocketAddr) -> Self {
        Self {
            socket: Arc::new(AsyncMutex::new(socket)),
            local_addr,
        }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    pub fn accept(&self) -> AcceptFuture {
        AcceptFuture {
            inner: Box::pin(accept_handshake(self.socket.clone())),
        }
    }
}

async fn accept_handshake(socket: Arc<AsyncMutex<UdpSocket>>) -> Option<Connecting> {
    let mut buf = [0u8; HANDSHAKE_PACKET_LEN];
    loop {
        let mut guard = socket.lock().await;
        match guard.recv_from(&mut buf).await {
            Ok((len, peer)) => {
                drop(guard);
                if len != HANDSHAKE_PACKET_LEN {
                    continue;
                }
                if &buf[..4] != &HANDSHAKE_MAGIC {
                    continue;
                }
                let mut token_bytes = [0u8; 8];
                token_bytes.copy_from_slice(&buf[4..12]);
                let token = u64::from_be_bytes(token_bytes);
                return Some(Connecting::new(socket.clone(), peer, token));
            }
            Err(_) => {
                record_handshake_fail("io_error");
                return None;
            }
        }
    }
}

pub struct AcceptFuture {
    inner: Pin<Box<dyn Future<Output = Option<Connecting>> + Send>>,
}

impl Future for AcceptFuture {
    type Output = Option<Connecting>;

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.as_mut().poll(cx)
    }
}

pub struct Connecting {
    inner: Pin<Box<dyn Future<Output = std::result::Result<Connection, io::Error>> + Send>>,
}

impl Connecting {
    fn new(socket: Arc<AsyncMutex<UdpSocket>>, peer: SocketAddr, token: u64) -> Self {
        let fut = async move {
            let mut guard = socket.lock().await;
            let mut ack = [0u8; HANDSHAKE_PACKET_LEN];
            ack[..4].copy_from_slice(&HANDSHAKE_ACK_MAGIC);
            ack[4..12].copy_from_slice(&token.to_be_bytes());
            guard.send_to(&ack, peer).await?;
            Ok(Connection { peer })
        };
        Self {
            inner: Box::pin(fut),
        }
    }
}

impl Future for Connecting {
    type Output = std::result::Result<Connection, io::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        self.get_mut().inner.as_mut().poll(cx)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Connection {
    peer: SocketAddr,
}

impl Connection {
    pub fn remote_addr(&self) -> SocketAddr {
        self.peer
    }
}

async fn wait_for_ack(
    socket: &mut UdpSocket,
    expected_addr: SocketAddr,
    expected_token: u64,
) -> io::Result<()> {
    let mut buf = [0u8; HANDSHAKE_PACKET_LEN];
    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        if addr != expected_addr {
            continue;
        }
        if len != HANDSHAKE_PACKET_LEN {
            continue;
        }
        if &buf[..4] != &HANDSHAKE_ACK_MAGIC {
            continue;
        }
        let mut token_bytes = [0u8; 8];
        token_bytes.copy_from_slice(&buf[4..12]);
        if u64::from_be_bytes(token_bytes) == expected_token {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::OsRng;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use sys::tempfile::TempDir;

    fn reset_state() {
        *cert_state().write().unwrap() = CertState::default();
    }

    #[test]
    fn verify_remote_certificate_matches_generated_advertisement() {
        let temp = TempDir::new().expect("temp dir");
        set_cert_store_path(Some(temp.path().join("quic-test.json")));
        reset_state();

        let mut secret = [0u8; 32];
        OsRng::default().fill_bytes(&mut secret);
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
