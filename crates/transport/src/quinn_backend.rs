use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::{ProviderCapability, RetryPolicy, SessionResumeStore};
use concurrency::Bytes;
use diagnostics::{anyhow, Result, TbError};
use foundation_lazy::sync::OnceCell;
use foundation_time::{Duration as TimeDuration, UtcDateTime};
use foundation_tls::{
    ed25519_public_key_from_der, generate_self_signed_ed25519, OcspResponse, RotationPolicy,
    SelfSignedCertParams, TrustAnchorStore,
};
use rand::thread_rng;
use rand::Rng;

use crate::inhouse::{
    self as inhouse_impl, ConnectOutcome as InhouseOutcome, InhouseEventCallbacks,
};

pub type Connection = Arc<inhouse_impl::Connection>;
pub type Endpoint = inhouse_impl::Endpoint;

#[derive(Clone, Default)]
pub struct QuinnEventCallbacks {
    pub handshake_latency: Option<Arc<dyn Fn(SocketAddr, Duration) + Send + Sync + 'static>>,
    pub handshake_failure: Option<Arc<dyn Fn(SocketAddr, HandshakeError) + Send + Sync + 'static>>,
    pub endpoint_reuse: Option<Arc<dyn Fn(SocketAddr) + Send + Sync + 'static>>,
    pub bytes_sent: Option<Arc<dyn Fn(SocketAddr, u64) + Send + Sync + 'static>>,
    pub bytes_received: Option<Arc<dyn Fn(SocketAddr, u64) + Send + Sync + 'static>>,
    pub disconnect: Option<Arc<dyn Fn(SocketAddr, QuinnDisconnect) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

#[derive(Debug)]
pub enum QuinnCallbackError {
    AlreadyInstalled,
}

#[derive(Debug)]
pub enum ConnectError {
    Handshake(HandshakeError),
    Other(TbError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Handshake(err) => write!(f, "handshake failed: {}", err.as_str()),
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ConnectError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeError {
    Tls,
    Version,
    Timeout,
    Certificate,
    Other,
}

impl HandshakeError {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandshakeError::Tls => "tls",
            HandshakeError::Version => "version",
            HandshakeError::Timeout => "timeout",
            HandshakeError::Certificate => "certificate",
            HandshakeError::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum QuinnDisconnect {
    ApplicationClosed { code: u64 },
    ConnectionClosed { code: u64 },
    Reset,
    TransportError { code: u64 },
    WriteStopped { code: u64 },
    WriteFailure,
}

impl QuinnDisconnect {
    pub fn label(&self) -> Cow<'static, str> {
        match self {
            QuinnDisconnect::ApplicationClosed { code }
            | QuinnDisconnect::ConnectionClosed { code }
            | QuinnDisconnect::TransportError { code }
            | QuinnDisconnect::WriteStopped { code } => Cow::Owned(code.to_string()),
            QuinnDisconnect::Reset => Cow::Borrowed("reset"),
            QuinnDisconnect::WriteFailure => Cow::Borrowed("write_failure"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ConnectionStatsSnapshot {
    pub lost_packets: u64,
    pub rtt: Duration,
}

pub const CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::CertificateRotation,
    ProviderCapability::ConnectionPooling,
    ProviderCapability::InsecureConnect,
    ProviderCapability::TelemetryCallbacks,
];

pub const PROVIDER_ID: &str = "quinn";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    inner: inhouse_impl::Certificate,
}

impl Certificate {
    pub fn from_der(der: impl Into<Bytes>) -> Self {
        let bytes = der.into();
        Self {
            inner: inhouse_impl::Certificate::from_der_lossy(bytes),
        }
    }

    pub fn der(&self) -> &Bytes {
        &self.inner.der
    }

    pub fn into_bytes(self) -> Bytes {
        self.inner.der
    }

    fn as_inhouse(&self) -> &inhouse_impl::Certificate {
        &self.inner
    }
}

impl From<Vec<u8>> for Certificate {
    fn from(value: Vec<u8>) -> Self {
        Certificate::from_der(value)
    }
}

impl From<&[u8]> for Certificate {
    fn from(value: &[u8]) -> Self {
        Certificate::from_der(value)
    }
}

struct QuinnState {
    adapter: inhouse_impl::Adapter,
    callbacks: Arc<QuinnEventCallbacks>,
    retry: RetryPolicy,
    handshake_timeout: Duration,
    trust_anchors: Option<Arc<TrustAnchorStore>>,
    session_store: Option<Arc<SessionResumeStore>>,
    ocsp: Option<OcspResponse>,
}

static STATE: OnceCell<RwLock<QuinnState>> = OnceCell::new();

fn state() -> &'static RwLock<QuinnState> {
    STATE.get_or_init(|| {
        let callbacks = Arc::new(QuinnEventCallbacks::default());
        let adapter = build_adapter(&callbacks, &RetryPolicy::default(), Duration::from_secs(5))
            .expect("initialize quinn adapter");
        RwLock::new(QuinnState {
            adapter,
            callbacks,
            retry: RetryPolicy::default(),
            handshake_timeout: Duration::from_secs(5),
            trust_anchors: None,
            session_store: None,
            ocsp: None,
        })
    })
}

fn build_adapter(
    callbacks: &Arc<QuinnEventCallbacks>,
    retry: &RetryPolicy,
    handshake_timeout: Duration,
) -> Result<inhouse_impl::Adapter> {
    let mut backend_callbacks = InhouseEventCallbacks::default();
    backend_callbacks.provider_connect = callbacks.provider_connect.clone();
    backend_callbacks.handshake_success = None;
    backend_callbacks.handshake_failure = None;
    inhouse_impl::Adapter::new(retry.clone(), handshake_timeout, &backend_callbacks)
}

fn with_state_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut QuinnState) -> R,
{
    let lock = state();
    let mut guard = lock.write().unwrap();
    f(&mut guard)
}

fn adapter_clone() -> inhouse_impl::Adapter {
    state().read().unwrap().adapter.clone()
}

fn callbacks_clone() -> Arc<QuinnEventCallbacks> {
    state().read().unwrap().callbacks.clone()
}

pub fn set_event_callbacks(
    callbacks: QuinnEventCallbacks,
) -> std::result::Result<(), QuinnCallbackError> {
    with_state_mut(|state| {
        state.callbacks = Arc::new(callbacks);
        state.adapter = build_adapter(&state.callbacks, &state.retry, state.handshake_timeout)
            .map_err(|_| QuinnCallbackError::AlreadyInstalled)?;
        Ok(())
    })
}

pub fn set_retry_policy(policy: RetryPolicy) {
    with_state_mut(|state| {
        state.retry = policy;
        if let Ok(adapter) = build_adapter(&state.callbacks, &state.retry, state.handshake_timeout)
        {
            state.adapter = adapter;
        }
    });
}

pub fn set_handshake_timeout(timeout: Duration) {
    with_state_mut(|state| {
        state.handshake_timeout = timeout;
        if let Ok(adapter) = build_adapter(&state.callbacks, &state.retry, state.handshake_timeout)
        {
            state.adapter = adapter;
        }
    });
}

pub fn install_trust_anchors(store: TrustAnchorStore) {
    with_state_mut(|state| {
        state.trust_anchors = Some(Arc::new(store));
    });
}

pub fn clear_trust_anchors() {
    with_state_mut(|state| {
        state.trust_anchors = None;
    });
}

pub fn trust_anchor_fingerprints() -> Vec<[u8; 32]> {
    state()
        .read()
        .unwrap()
        .trust_anchors
        .as_ref()
        .map(|store| store.fingerprints())
        .unwrap_or_default()
}

pub fn install_session_store(store: SessionResumeStore) {
    with_state_mut(|state| {
        state.session_store = Some(Arc::new(store));
    });
}

pub fn clear_session_store() {
    with_state_mut(|state| {
        state.session_store = None;
    });
}

pub fn install_ocsp_response(response: OcspResponse) {
    with_state_mut(|state| {
        state.ocsp = Some(response);
    });
}

pub fn clear_ocsp_response() {
    with_state_mut(|state| {
        state.ocsp = None;
    });
}

pub async fn listen(addr: SocketAddr) -> Result<(Endpoint, Certificate)> {
    let adapter = adapter_clone();
    let certificate = generate_listening_certificate(addr)?;
    let (endpoint, cert) = adapter
        .listen_with_certificate(addr, certificate.inner.clone())
        .await?;
    Ok((endpoint, Certificate { inner: cert }))
}

pub async fn listen_with_cert(
    addr: SocketAddr,
    cert_der: &Bytes,
    key_der: &Bytes,
) -> Result<Endpoint> {
    listen_with_chain(addr, std::slice::from_ref(cert_der), key_der).await
}

pub async fn listen_with_chain(
    addr: SocketAddr,
    chain: &[Bytes],
    key_der: &Bytes,
) -> Result<Endpoint> {
    if chain.is_empty() {
        return Err(anyhow!("certificate chain must not be empty"));
    }
    let certificate = Certificate::from_der(chain[0].clone());
    validate_private_key(&certificate, key_der)?;
    let adapter = adapter_clone();
    let (endpoint, _) = adapter
        .listen_with_certificate(addr, certificate.inner.clone())
        .await?;
    Ok(endpoint)
}

pub async fn connect(
    addr: SocketAddr,
    cert: &Certificate,
) -> std::result::Result<Connection, ConnectError> {
    let adapter = adapter_clone();
    connect_impl(&adapter, addr, cert.as_inhouse()).await
}

pub async fn get_connection(
    addr: SocketAddr,
    cert: &Certificate,
) -> std::result::Result<Connection, ConnectError> {
    let adapter = adapter_clone();
    connect_impl(&adapter, addr, cert.as_inhouse()).await
}

pub fn drop_connection(addr: &SocketAddr) {
    adapter_clone().drop_connection(addr);
}

pub fn connection_stats() -> Vec<(SocketAddr, ConnectionStatsSnapshot)> {
    adapter_clone()
        .connection_stats()
        .into_iter()
        .map(|(addr, stats)| {
            (
                addr,
                ConnectionStatsSnapshot {
                    lost_packets: stats.retransmits,
                    rtt: stats.handshake_latency,
                },
            )
        })
        .collect()
}

pub async fn send(conn: &Connection, data: &[u8]) -> Result<()> {
    let mut rng = thread_rng();
    if let Ok(loss_str) = std::env::var("TB_QUIC_PACKET_LOSS") {
        if let Ok(loss) = loss_str.parse::<f64>() {
            if rng.gen_bool(loss) {
                return Ok(());
            }
        }
    }
    adapter_clone().send(conn, data).await.map_err(|err| {
        notify_disconnect(conn.peer_addr(), QuinnDisconnect::WriteFailure);
        err
    })?;
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.bytes_sent.as_ref() {
            cb(conn.peer_addr(), data.len() as u64);
        }
    });
    if let Ok(dup_str) = std::env::var("TB_QUIC_PACKET_DUP") {
        if let Ok(dup) = dup_str.parse::<f64>() {
            if rng.gen_bool(dup) {
                let _ = adapter_clone().send(conn, data).await;
            }
        }
    }
    Ok(())
}

pub async fn recv(conn: &Connection) -> Option<Vec<u8>> {
    let payload = adapter_clone().recv(conn).await;
    if let Some(buf) = payload.as_ref() {
        with_callbacks(|cbs| {
            if let Some(cb) = cbs.bytes_received.as_ref() {
                cb(conn.peer_addr(), buf.len() as u64);
            }
        });
    }
    payload
}

pub async fn connect_insecure(addr: SocketAddr) -> std::result::Result<Connection, ConnectError> {
    let adapter = adapter_clone();
    match adapter.connect_insecure(addr).await {
        Ok((conn, outcome)) => {
            handle_outcome(addr, &outcome);
            Ok(conn)
        }
        Err(err) => Err(handle_connect_error(addr, err)),
    }
}

pub fn classify_err(err: &ConnectError) -> HandshakeError {
    match err {
        ConnectError::Handshake(kind) => *kind,
        ConnectError::Other(_) => HandshakeError::Other,
    }
}

fn generate_listening_certificate(addr: SocketAddr) -> Result<Certificate> {
    let now = UtcDateTime::now();
    let anchor =
        UtcDateTime::from_unix_timestamp(0).map_err(|_| anyhow!("unix epoch unavailable"))?;
    let policy = RotationPolicy::new(anchor, TimeDuration::days(7), TimeDuration::hours(1))
        .map_err(|err| anyhow!("invalid rotation policy: {err}"))?;
    let slot = policy
        .slot_at(now)
        .map_err(|err| anyhow!("compute rotation slot failed: {err}"))?;
    let context = format!("quinn:{addr}");
    let plan = policy
        .plan(slot, context.as_bytes())
        .map_err(|err| anyhow!("plan rotation window failed: {err}"))?;
    let params = SelfSignedCertParams::builder()
        .subject_cn("the-block quinn listener")
        .add_dns_name("the-block")
        .apply_rotation_plan(&plan)
        .build()
        .map_err(|err| anyhow!("build certificate params failed: {err}"))?;
    let generated = generate_self_signed_ed25519(&params)
        .map_err(|err| anyhow!("generate certificate failed: {err}"))?;
    Ok(Certificate::from_der(Bytes::from(generated.certificate)))
}

fn validate_private_key(cert: &Certificate, key_der: &Bytes) -> Result<()> {
    let signing_key = crypto_suite::signatures::ed25519::SigningKey::from_pkcs8_der(key_der)
        .map_err(|err| anyhow!("decode private key: {err}"))?;
    let verifying = signing_key.verifying_key().to_bytes();
    let expected = ed25519_public_key_from_der(cert.der())
        .map_err(|err| anyhow!("certificate parse failed: {err}"))?;
    if verifying != expected {
        return Err(anyhow!("private key does not match certificate"));
    }
    Ok(())
}

async fn connect_impl(
    adapter: &inhouse_impl::Adapter,
    addr: SocketAddr,
    cert: &inhouse_impl::Certificate,
) -> std::result::Result<Connection, ConnectError> {
    match adapter.connect(addr, cert).await {
        Ok((conn, outcome)) => {
            handle_outcome(addr, &outcome);
            Ok(conn)
        }
        Err(err) => Err(handle_connect_error(addr, err)),
    }
}

fn handle_outcome(addr: SocketAddr, outcome: &InhouseOutcome) {
    with_callbacks(|cbs| {
        if outcome.reused {
            if let Some(cb) = cbs.endpoint_reuse.as_ref() {
                cb(addr);
            }
        } else {
            if let Some(cb) = cbs.handshake_latency.as_ref() {
                cb(addr, outcome.handshake_latency);
            }
            if let Some(cb) = cbs.provider_connect.as_ref() {
                cb(PROVIDER_ID);
            }
        }
    });
}

fn handle_connect_error(addr: SocketAddr, err: TbError) -> ConnectError {
    let classification = classify_tb_error(&err);
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.handshake_failure.as_ref() {
            cb(addr, classification);
        }
    });
    ConnectError::Handshake(classification)
}

fn classify_tb_error(err: &TbError) -> HandshakeError {
    let msg = err.to_string().to_lowercase();
    if msg.contains("timeout") {
        HandshakeError::Timeout
    } else if msg.contains("certificate") {
        HandshakeError::Certificate
    } else if msg.contains("tls") {
        HandshakeError::Tls
    } else if msg.contains("version") {
        HandshakeError::Version
    } else {
        HandshakeError::Other
    }
}

fn notify_disconnect(addr: SocketAddr, disconnect: QuinnDisconnect) {
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.disconnect.as_ref() {
            cb(addr, disconnect);
        }
    });
}

fn with_callbacks<F>(f: F)
where
    F: FnOnce(&QuinnEventCallbacks),
{
    let callbacks = callbacks_clone();
    f(callbacks.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_error_variants() {
        let timeout = ConnectError::Handshake(HandshakeError::Timeout);
        assert_eq!(classify_err(&timeout), HandshakeError::Timeout);
        let other = ConnectError::Other(anyhow!("other"));
        assert_eq!(classify_err(&other), HandshakeError::Other);
    }
}
