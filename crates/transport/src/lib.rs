#![allow(
    clippy::collapsible_if,
    clippy::needless_lifetimes,
    clippy::manual_is_ascii_check,
    clippy::type_complexity,
    clippy::field_reassign_with_default,
    clippy::useless_conversion,
    clippy::needless_question_mark,
    clippy::vec_init_then_push,
    clippy::manual_inspect,
    clippy::clone_on_copy,
    clippy::needless_borrows_for_generic_args,
    clippy::op_ref
)]

#[cfg(all(feature = "quinn", not(feature = "s2n-quic")))]
use crypto_suite::hashing::blake3;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

#[cfg(any(feature = "quinn", feature = "inhouse"))]
use concurrency::Bytes;

#[cfg(any(feature = "quinn", feature = "s2n-quic"))]
mod session;

#[cfg(any(feature = "quinn", feature = "s2n-quic"))]
pub use session::SessionResumeStore;

#[cfg(any(feature = "quinn", feature = "s2n-quic"))]
use foundation_tls::{OcspResponse, TrustAnchorStore};

#[cfg(feature = "s2n-quic")]
mod cert_parser;

#[cfg(feature = "s2n-quic")]
use crypto_suite::signatures::ed25519::SigningKey;

#[cfg(feature = "inhouse")]
use crate::inhouse as inhouse_impl;

/// Known transport provider implementations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    Quinn,
    S2nQuic,
    #[cfg(feature = "inhouse")]
    Inhouse,
}

impl ProviderKind {
    pub fn id(&self) -> &'static str {
        match self {
            ProviderKind::Quinn => {
                #[cfg(feature = "quinn")]
                {
                    quinn_impl::PROVIDER_ID
                }
                #[cfg(not(feature = "quinn"))]
                {
                    "quinn"
                }
            }
            ProviderKind::S2nQuic => {
                #[cfg(feature = "s2n-quic")]
                {
                    s2n_impl::PROVIDER_ID
                }
                #[cfg(not(feature = "s2n-quic"))]
                {
                    "s2n-quic"
                }
            }
            #[cfg(feature = "inhouse")]
            ProviderKind::Inhouse => inhouse_impl::PROVIDER_ID,
        }
    }
}

/// High level capabilities a transport provider may expose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderCapability {
    CertificateRotation,
    ConnectionPooling,
    InsecureConnect,
    TelemetryCallbacks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderMetadata {
    pub kind: ProviderKind,
    pub id: &'static str,
    pub capabilities: &'static [ProviderCapability],
}

/// Generic QUIC listener interface implemented by backends.
pub trait QuicListener {
    type Endpoint;
    type Error;

    fn listen<'a>(
        &'a self,
        addr: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Endpoint, Self::Error>> + Send + 'a>>;
}

/// Generic QUIC connector interface implemented by backends.
pub trait QuicConnector {
    type Connection;
    type Error;

    fn connect<'a>(
        &'a self,
        addr: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Connection, Self::Error>> + Send + 'a>>;
}

/// Certificate store operations required by the node when managing TLS material.
pub trait CertificateStore {
    type Advertisement;
    type Error;

    fn initialize(&self) -> Result<Self::Advertisement, Self::Error>;
    fn rotate(&self) -> Result<Self::Advertisement, Self::Error>;
    fn current(&self) -> Option<Self::Advertisement>;
}

/// Retry policy used by transport providers when binding sockets or establishing
/// connections.
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub attempts: usize,
    pub backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            backoff: Duration::from_millis(50),
        }
    }
}

/// TLS material that providers can install during initialization.
#[derive(Clone, Default)]
pub struct TlsSettings {
    #[cfg(any(feature = "quinn", feature = "s2n-quic"))]
    pub trust_anchors: Option<TrustAnchorStore>,
    #[cfg(any(feature = "quinn", feature = "s2n-quic"))]
    pub session_store: Option<SessionResumeStore>,
    #[cfg(any(feature = "quinn", feature = "s2n-quic"))]
    pub ocsp: Option<OcspResponse>,
}

impl fmt::Debug for TlsSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("TlsSettings");
        #[cfg(any(feature = "quinn", feature = "s2n-quic"))]
        {
            debug.field(
                "trust_anchors",
                &self.trust_anchors.as_ref().map(|store| store.len()),
            );
            debug.field(
                "session_store",
                &self.session_store.as_ref().map(|_| "present"),
            );
            debug.field("ocsp", &self.ocsp.as_ref().map(|_| "present"));
        }
        debug.finish()
    }
}

/// Common configuration for the transport abstraction.
#[derive(Clone, Debug)]
pub struct Config {
    pub provider: ProviderKind,
    pub certificate_cache: Option<PathBuf>,
    pub retry: RetryPolicy,
    pub handshake_timeout: Duration,
    pub tls: TlsSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: DEFAULT_PROVIDER_KIND,
            certificate_cache: None,
            retry: RetryPolicy::default(),
            handshake_timeout: Duration::from_secs(5),
            tls: TlsSettings::default(),
        }
    }
}

#[cfg(feature = "inhouse")]
const DEFAULT_PROVIDER_KIND: ProviderKind = ProviderKind::Inhouse;

#[cfg(all(not(feature = "inhouse"), feature = "quinn"))]
const DEFAULT_PROVIDER_KIND: ProviderKind = ProviderKind::Quinn;

#[cfg(all(not(feature = "inhouse"), not(feature = "quinn"), feature = "s2n-quic"))]
const DEFAULT_PROVIDER_KIND: ProviderKind = ProviderKind::S2nQuic;

#[cfg(all(
    not(feature = "inhouse"),
    not(feature = "quinn"),
    not(feature = "s2n-quic")
))]
compile_error!("transport crate compiled without any QUIC providers");

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
use diagnostics::{anyhow, Result as DiagResult};

#[cfg(feature = "quinn")]
use crate::quinn_backend as quinn_impl;

#[cfg(feature = "s2n-quic")]
use crate::s2n_backend as s2n_impl;

/// Callback hooks that the node can install on active providers.
#[cfg(feature = "quinn")]
#[derive(Clone, Default)]
pub struct QuinnCallbacks {
    pub handshake_latency: Option<Arc<dyn Fn(SocketAddr, Duration) + Send + Sync + 'static>>,
    pub handshake_failure:
        Option<Arc<dyn Fn(SocketAddr, quinn_impl::HandshakeError) + Send + Sync + 'static>>,
    pub endpoint_reuse: Option<Arc<dyn Fn(SocketAddr) + Send + Sync + 'static>>,
    pub bytes_sent: Option<Arc<dyn Fn(SocketAddr, u64) + Send + Sync + 'static>>,
    pub bytes_received: Option<Arc<dyn Fn(SocketAddr, u64) + Send + Sync + 'static>>,
    pub disconnect:
        Option<Arc<dyn Fn(SocketAddr, quinn_impl::QuinnDisconnect) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

#[cfg(not(feature = "quinn"))]
#[derive(Clone, Default)]
pub struct QuinnCallbacks;

#[cfg(feature = "inhouse")]
#[derive(Clone, Default)]
pub struct InhouseCallbacks {
    pub handshake_success: Option<Arc<dyn Fn(SocketAddr) + Send + Sync + 'static>>,
    pub handshake_failure: Option<Arc<dyn Fn(SocketAddr, &str) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

#[cfg(not(feature = "inhouse"))]
#[derive(Clone, Default)]
pub struct InhouseCallbacks;

/// Callback hooks for s2n-quic providers.
#[derive(Clone, Default)]
pub struct S2nCallbacks {
    pub cert_rotated: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
    pub handshake_failure: Option<Arc<dyn Fn(&str) + Send + Sync + 'static>>,
    pub retransmit: Option<Arc<dyn Fn(u64) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

/// Provider callback collection used when instantiating the registry.
#[derive(Clone, Default)]
pub struct TransportCallbacks {
    pub quinn: QuinnCallbacks,
    pub inhouse: InhouseCallbacks,
    pub s2n: S2nCallbacks,
}

/// Factory responsible for constructing provider registries.
#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
pub trait TransportFactory: Send + Sync {
    fn create(&self, cfg: &Config, callbacks: &TransportCallbacks) -> DiagResult<ProviderRegistry>;
}

/// Default factory wiring the concrete backend implementations.
#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
#[derive(Clone, Default)]
pub struct DefaultFactory;

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
impl TransportFactory for DefaultFactory {
    fn create(&self, cfg: &Config, callbacks: &TransportCallbacks) -> DiagResult<ProviderRegistry> {
        let instance = match cfg.provider {
            ProviderKind::Quinn => {
                #[cfg(feature = "quinn")]
                {
                    apply_quinn_tls(&cfg.tls);
                    #[cfg(feature = "s2n-quic")]
                    reset_s2n_tls();
                    ProviderInstance::new_quinn(cfg, &callbacks.quinn)?
                }
                #[cfg(not(feature = "quinn"))]
                {
                    return Err(anyhow!("quinn provider not compiled"));
                }
            }
            ProviderKind::S2nQuic => {
                #[cfg(feature = "s2n-quic")]
                {
                    #[cfg(feature = "quinn")]
                    reset_quinn_tls();
                    apply_s2n_tls(&cfg.tls);
                    ProviderInstance::new_s2n(cfg, &callbacks.s2n)?
                }
                #[cfg(not(feature = "s2n-quic"))]
                {
                    return Err(anyhow!("s2n-quic provider not compiled"));
                }
            }
            #[cfg(feature = "inhouse")]
            ProviderKind::Inhouse => {
                #[cfg(feature = "quinn")]
                reset_quinn_tls();
                #[cfg(feature = "s2n-quic")]
                reset_s2n_tls();
                ProviderInstance::new_inhouse(cfg, &callbacks.inhouse)?
            }
        };
        Ok(ProviderRegistry {
            inner: Arc::new(instance),
        })
    }
}

#[cfg(feature = "quinn")]
fn apply_quinn_tls(settings: &TlsSettings) {
    match settings.trust_anchors.clone() {
        Some(store) => quinn_impl::install_trust_anchors(store),
        None => quinn_impl::clear_trust_anchors(),
    }
    match settings.session_store.clone() {
        Some(cache) => quinn_impl::install_session_store(cache),
        None => quinn_impl::clear_session_store(),
    }
    match settings.ocsp.clone() {
        Some(response) => quinn_impl::install_ocsp_response(response),
        None => quinn_impl::clear_ocsp_response(),
    }
}

#[cfg(all(feature = "quinn", any(feature = "s2n-quic", feature = "inhouse")))]
fn reset_quinn_tls() {
    quinn_impl::clear_trust_anchors();
    quinn_impl::clear_session_store();
    quinn_impl::clear_ocsp_response();
}

#[cfg(feature = "s2n-quic")]
fn apply_s2n_tls(settings: &TlsSettings) {
    match settings.trust_anchors.clone() {
        Some(store) => s2n_impl::install_trust_anchors(store),
        None => s2n_impl::clear_trust_anchors(),
    }
    match settings.session_store.clone() {
        Some(cache) => s2n_impl::install_session_store(cache),
        None => s2n_impl::clear_session_store(),
    }
    match settings.ocsp.clone() {
        Some(response) => s2n_impl::install_ocsp_response(response),
        None => s2n_impl::clear_ocsp_response(),
    }
}

#[cfg(all(feature = "s2n-quic", any(feature = "quinn", feature = "inhouse")))]
fn reset_s2n_tls() {
    s2n_impl::clear_trust_anchors();
    s2n_impl::clear_session_store();
    s2n_impl::clear_ocsp_response();
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
#[derive(Clone)]
pub struct ProviderRegistry {
    inner: Arc<ProviderInstance>,
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
impl ProviderRegistry {
    pub fn kind(&self) -> ProviderKind {
        self.inner.kind()
    }

    pub fn provider_id(&self) -> &'static str {
        self.inner.metadata().id
    }

    pub fn metadata(&self) -> ProviderMetadata {
        self.inner.metadata()
    }

    pub fn capabilities(&self) -> &'static [ProviderCapability] {
        self.inner.capabilities()
    }

    #[cfg(feature = "s2n-quic")]
    pub fn fingerprint_history(&self) -> Option<Vec<[u8; 32]>> {
        match self.inner.as_ref() {
            ProviderInstance::S2n(adapter) => Some(adapter.fingerprint_history()),
            #[cfg(any(feature = "quinn", feature = "inhouse"))]
            _ => None,
        }
    }

    #[cfg(feature = "quinn")]
    pub fn quinn(&self) -> Option<QuinnAdapter> {
        match self.inner.as_ref() {
            ProviderInstance::Quinn(adapter) => Some(adapter.clone()),
            #[cfg(feature = "s2n-quic")]
            ProviderInstance::S2n(_) => None,
            #[cfg(feature = "inhouse")]
            ProviderInstance::Inhouse(_) => None,
        }
    }

    #[cfg(feature = "s2n-quic")]
    pub fn s2n(&self) -> Option<S2nAdapter> {
        match self.inner.as_ref() {
            ProviderInstance::S2n(adapter) => Some(adapter.clone()),
            #[cfg(any(feature = "quinn", feature = "inhouse"))]
            _ => None,
        }
    }

    #[cfg(feature = "inhouse")]
    #[allow(irrefutable_let_patterns)]
    pub fn inhouse(&self) -> Option<InhouseAdapter> {
        if let ProviderInstance::Inhouse(adapter) = self.inner.as_ref() {
            Some(adapter.clone())
        } else {
            None
        }
    }
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
#[derive(Clone)]
enum ProviderInstance {
    #[cfg(feature = "quinn")]
    Quinn(QuinnAdapter),
    #[cfg(feature = "s2n-quic")]
    S2n(S2nAdapter),
    #[cfg(feature = "inhouse")]
    Inhouse(InhouseAdapter),
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
impl ProviderInstance {
    #[cfg(feature = "quinn")]
    fn new_quinn(cfg: &Config, callbacks: &QuinnCallbacks) -> DiagResult<Self> {
        QuinnAdapter::new(cfg, callbacks).map(Self::Quinn)
    }

    #[cfg(feature = "s2n-quic")]
    fn new_s2n(cfg: &Config, callbacks: &S2nCallbacks) -> DiagResult<Self> {
        S2nAdapter::new(cfg, callbacks).map(Self::S2n)
    }

    #[cfg(feature = "inhouse")]
    fn new_inhouse(cfg: &Config, callbacks: &InhouseCallbacks) -> DiagResult<Self> {
        let adapter = InhouseAdapter::new(cfg, callbacks)?;
        Ok(Self::Inhouse(adapter))
    }

    fn kind(&self) -> ProviderKind {
        match self {
            #[cfg(feature = "quinn")]
            ProviderInstance::Quinn(_) => ProviderKind::Quinn,
            #[cfg(feature = "s2n-quic")]
            ProviderInstance::S2n(_) => ProviderKind::S2nQuic,
            #[cfg(feature = "inhouse")]
            ProviderInstance::Inhouse(_) => ProviderKind::Inhouse,
        }
    }

    fn metadata(&self) -> ProviderMetadata {
        match self {
            #[cfg(feature = "quinn")]
            ProviderInstance::Quinn(_) => ProviderMetadata {
                kind: ProviderKind::Quinn,
                id: quinn_impl::PROVIDER_ID,
                capabilities: quinn_impl::CAPABILITIES,
            },
            #[cfg(feature = "s2n-quic")]
            ProviderInstance::S2n(_) => ProviderMetadata {
                kind: ProviderKind::S2nQuic,
                id: s2n_impl::PROVIDER_ID,
                capabilities: s2n_impl::CAPABILITIES,
            },
            #[cfg(feature = "inhouse")]
            ProviderInstance::Inhouse(adapter) => adapter.metadata(),
        }
    }

    fn capabilities(&self) -> &'static [ProviderCapability] {
        match self {
            #[cfg(feature = "quinn")]
            ProviderInstance::Quinn(_) => quinn_impl::CAPABILITIES,
            #[cfg(feature = "s2n-quic")]
            ProviderInstance::S2n(_) => s2n_impl::CAPABILITIES,
            #[cfg(feature = "inhouse")]
            ProviderInstance::Inhouse(_) => inhouse_impl::CAPABILITIES,
        }
    }
}

#[cfg(feature = "quinn")]
#[derive(Clone)]
pub struct QuinnAdapter(Arc<QuinnAdapterInner>);

#[cfg(feature = "quinn")]
struct QuinnAdapterInner {
    retry: RetryPolicy,
}

#[cfg(feature = "quinn")]
impl QuinnAdapter {
    fn new(cfg: &Config, callbacks: &QuinnCallbacks) -> DiagResult<Self> {
        let mut backend_callbacks = quinn_impl::QuinnEventCallbacks::default();
        backend_callbacks.handshake_latency = callbacks.handshake_latency.clone();
        backend_callbacks.handshake_failure = callbacks.handshake_failure.clone();
        backend_callbacks.endpoint_reuse = callbacks.endpoint_reuse.clone();
        backend_callbacks.bytes_sent = callbacks.bytes_sent.clone();
        backend_callbacks.bytes_received = callbacks.bytes_received.clone();
        backend_callbacks.disconnect = callbacks.disconnect.clone();
        backend_callbacks.provider_connect = callbacks.provider_connect.clone();
        quinn_impl::set_event_callbacks(backend_callbacks)
            .map_err(|_| anyhow!("quinn callbacks already installed"))?;
        quinn_impl::set_retry_policy(cfg.retry.clone());
        quinn_impl::set_handshake_timeout(cfg.handshake_timeout);
        Ok(Self(Arc::new(QuinnAdapterInner {
            retry: cfg.retry.clone(),
        })))
    }

    pub fn retry_policy(&self) -> RetryPolicy {
        self.0.retry.clone()
    }

    pub async fn listen(
        &self,
        addr: SocketAddr,
    ) -> DiagResult<(ListenerHandle, CertificateHandle)> {
        let (endpoint, cert) = quinn_impl::listen(addr).await?;
        Ok((
            ListenerHandle::Quinn(endpoint),
            CertificateHandle::Quinn(cert),
        ))
    }

    pub async fn listen_with_cert(
        &self,
        addr: SocketAddr,
        cert_der: Bytes,
        key_der: Bytes,
    ) -> DiagResult<ListenerHandle> {
        let endpoint = quinn_impl::listen_with_cert(addr, &cert_der, &key_der).await?;
        Ok(ListenerHandle::Quinn(endpoint))
    }

    pub async fn listen_with_chain(
        &self,
        addr: SocketAddr,
        chain: &[Bytes],
        key_der: Bytes,
    ) -> DiagResult<ListenerHandle> {
        let endpoint = quinn_impl::listen_with_chain(addr, chain, &key_der).await?;
        Ok(ListenerHandle::Quinn(endpoint))
    }

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &CertificateHandle,
    ) -> Result<ConnectionHandle, quinn_impl::ConnectError> {
        let cert = match cert {
            CertificateHandle::Quinn(cert) => cert,
            #[cfg(feature = "inhouse")]
            CertificateHandle::Inhouse(_) => {
                return Err(quinn_impl::ConnectError::Other(anyhow!(
                    "certificate incompatible with quinn provider"
                )))
            }
        };
        let conn = quinn_impl::connect(addr, cert).await?;
        Ok(ConnectionHandle::Quinn(conn))
    }

    pub async fn get_connection(
        &self,
        addr: SocketAddr,
        cert: &CertificateHandle,
    ) -> Result<ConnectionHandle, quinn_impl::ConnectError> {
        let cert = match cert {
            CertificateHandle::Quinn(cert) => cert,
            #[cfg(feature = "inhouse")]
            CertificateHandle::Inhouse(_) => {
                return Err(quinn_impl::ConnectError::Other(anyhow!(
                    "certificate incompatible with quinn provider"
                )))
            }
        };
        let conn = quinn_impl::get_connection(addr, cert).await?;
        Ok(ConnectionHandle::Quinn(conn))
    }

    pub fn drop_connection(&self, addr: &SocketAddr) {
        quinn_impl::drop_connection(addr);
    }

    pub fn connection_stats(&self) -> Vec<(SocketAddr, quinn_impl::ConnectionStatsSnapshot)> {
        quinn_impl::connection_stats()
    }

    pub async fn send(&self, conn: &ConnectionHandle, data: &[u8]) -> DiagResult<()> {
        match conn {
            ConnectionHandle::Quinn(conn) => quinn_impl::send(conn, data).await.map_err(Into::into),
            #[cfg(feature = "inhouse")]
            ConnectionHandle::Inhouse(_) => {
                Err(anyhow!("connection incompatible with quinn provider"))
            }
        }
    }

    pub async fn recv(&self, conn: &ConnectionHandle) -> Option<Vec<u8>> {
        match conn {
            ConnectionHandle::Quinn(conn) => quinn_impl::recv(conn).await,
            #[cfg(feature = "inhouse")]
            ConnectionHandle::Inhouse(_) => None,
        }
    }

    pub fn certificate_from_der(&self, cert: Bytes) -> CertificateHandle {
        CertificateHandle::Quinn(quinn_impl::Certificate::from_der(cert))
    }
}

#[cfg(feature = "inhouse")]
#[derive(Clone)]
pub struct InhouseAdapter(Arc<InhouseAdapterInner>);

#[cfg(feature = "inhouse")]
struct InhouseAdapterInner {
    backend: inhouse_impl::Adapter,
    retry: RetryPolicy,
}

#[cfg(feature = "inhouse")]
impl InhouseAdapter {
    fn new(cfg: &Config, callbacks: &InhouseCallbacks) -> DiagResult<Self> {
        let mut backend_callbacks = inhouse_impl::InhouseEventCallbacks::default();
        backend_callbacks.handshake_success = callbacks.handshake_success.clone();
        backend_callbacks.handshake_failure = callbacks.handshake_failure.clone();
        backend_callbacks.provider_connect = callbacks.provider_connect.clone();
        let adapter = inhouse_impl::Adapter::new(
            cfg.retry.clone(),
            cfg.handshake_timeout,
            &backend_callbacks,
        )?;
        Ok(Self(Arc::new(InhouseAdapterInner {
            backend: adapter,
            retry: cfg.retry.clone(),
        })))
    }

    pub fn metadata(&self) -> ProviderMetadata {
        self.0.backend.metadata()
    }

    pub fn retry_policy(&self) -> RetryPolicy {
        self.0.retry.clone()
    }

    pub async fn listen(
        &self,
        addr: SocketAddr,
    ) -> DiagResult<(ListenerHandle, CertificateHandle)> {
        let (endpoint, cert) = self.0.backend.listen(addr).await?;
        Ok((
            ListenerHandle::Inhouse(endpoint),
            CertificateHandle::Inhouse(cert),
        ))
    }

    pub async fn listen_with_certificate(
        &self,
        addr: SocketAddr,
        certificate: inhouse_impl::Certificate,
    ) -> DiagResult<(ListenerHandle, CertificateHandle)> {
        let (endpoint, cert) = self
            .0
            .backend
            .listen_with_certificate(addr, certificate)
            .await?;
        Ok((
            ListenerHandle::Inhouse(endpoint),
            CertificateHandle::Inhouse(cert),
        ))
    }

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &CertificateHandle,
    ) -> DiagResult<ConnectionHandle> {
        let cert = match cert {
            CertificateHandle::Inhouse(cert) => cert,
            #[cfg(feature = "quinn")]
            CertificateHandle::Quinn(_) => {
                return Err(anyhow!("certificate incompatible with inhouse provider"));
            }
        };
        let (conn, _meta) = self.0.backend.connect(addr, cert).await?;
        Ok(ConnectionHandle::Inhouse(conn))
    }

    pub fn drop_connection(&self, addr: &SocketAddr) {
        self.0.backend.drop_connection(addr);
    }

    pub fn connection_stats(&self) -> Vec<(SocketAddr, inhouse_impl::ConnectionStatsSnapshot)> {
        self.0.backend.connection_stats()
    }

    pub async fn send(&self, conn: &ConnectionHandle, data: &[u8]) -> DiagResult<()> {
        match conn {
            ConnectionHandle::Inhouse(conn) => {
                self.0.backend.send(conn, data).await?;
                Ok(())
            }
            #[cfg(feature = "quinn")]
            ConnectionHandle::Quinn(_) => {
                Err(anyhow!("connection incompatible with inhouse provider"))
            }
        }
    }

    pub async fn recv(&self, conn: &ConnectionHandle) -> Option<Vec<u8>> {
        match conn {
            ConnectionHandle::Inhouse(conn) => self.0.backend.recv(conn).await,
            #[cfg(feature = "quinn")]
            ConnectionHandle::Quinn(_) => None,
        }
    }

    pub fn verify_remote_certificate(
        &self,
        peer_key: &[u8; 32],
        cert: &[u8],
    ) -> DiagResult<[u8; 32]> {
        Ok(self.0.backend.verify_remote_certificate(peer_key, cert)?)
    }

    pub fn certificate_from_der(&self, cert: Bytes) -> CertificateHandle {
        let certificate = inhouse_impl::Certificate::from_der_lossy(cert);
        CertificateHandle::Inhouse(certificate)
    }
}

#[cfg(feature = "s2n-quic")]
#[derive(Clone)]
pub struct S2nAdapter(Arc<S2nAdapterInner>);

#[cfg(feature = "s2n-quic")]
struct S2nAdapterInner {
    certificate_cache: Option<PathBuf>,
}

#[cfg(feature = "s2n-quic")]
impl S2nAdapter {
    fn new(cfg: &Config, callbacks: &S2nCallbacks) -> DiagResult<Self> {
        let mut backend_callbacks = s2n_impl::S2nEventCallbacks::default();
        backend_callbacks.cert_rotated = callbacks.cert_rotated.clone();
        backend_callbacks.handshake_failure = callbacks.handshake_failure.clone();
        backend_callbacks.retransmit = callbacks.retransmit.clone();
        backend_callbacks.provider_connect = callbacks.provider_connect.clone();
        s2n_impl::set_event_callbacks(backend_callbacks)
            .map_err(|_| anyhow!("s2n callbacks already installed"))?;
        s2n_impl::set_cert_store_path(cfg.certificate_cache.clone());
        s2n_impl::set_handshake_timeout(cfg.handshake_timeout);
        Ok(Self(Arc::new(S2nAdapterInner {
            certificate_cache: cfg.certificate_cache.clone(),
        })))
    }

    pub fn certificate_cache(&self) -> Option<PathBuf> {
        self.0.certificate_cache.clone()
    }

    pub fn initialize(&self, signing_key: &SigningKey) -> DiagResult<s2n_impl::CertAdvertisement> {
        s2n_impl::initialize(signing_key)
    }

    pub fn rotate(&self, signing_key: &SigningKey) -> DiagResult<s2n_impl::CertAdvertisement> {
        s2n_impl::rotate(signing_key)
    }

    pub fn current_cert(&self) -> Option<s2n_impl::LocalCert> {
        s2n_impl::current_cert()
    }

    pub fn current_advertisement(&self) -> Option<s2n_impl::CertAdvertisement> {
        s2n_impl::current_advertisement()
    }

    pub fn fingerprint_history(&self) -> Vec<[u8; 32]> {
        s2n_impl::fingerprint_history()
    }

    pub fn fingerprint(&self, cert: &[u8]) -> [u8; 32] {
        s2n_impl::fingerprint(cert)
    }

    pub fn verify_remote_certificate(
        &self,
        peer_key: &[u8; 32],
        cert: &[u8],
    ) -> DiagResult<[u8; 32]> {
        s2n_impl::verify_remote_certificate(peer_key, cert)
    }

    pub async fn start_server(
        &self,
        addr: SocketAddr,
        signing_key: &SigningKey,
    ) -> DiagResult<ListenerHandle> {
        let server = s2n_impl::start_server(addr, signing_key).await?;
        Ok(ListenerHandle::S2n(server))
    }

    pub async fn connect(&self, addr: SocketAddr) -> DiagResult<()> {
        s2n_impl::connect(addr).await
    }

    pub fn record_handshake_fail(&self, reason: &str) {
        s2n_impl::record_handshake_fail(reason);
    }

    pub fn record_retransmit(&self, count: u64) {
        s2n_impl::record_retransmit(count);
    }
}

#[cfg(any(feature = "quinn", feature = "inhouse"))]
#[derive(Clone)]
pub enum ConnectionHandle {
    #[cfg(feature = "quinn")]
    Quinn(quinn_impl::Connection),
    #[cfg(feature = "inhouse")]
    Inhouse(Arc<inhouse_impl::Connection>),
}

#[cfg(any(feature = "quinn", feature = "inhouse"))]
#[derive(Clone)]
pub enum CertificateHandle {
    #[cfg(feature = "quinn")]
    Quinn(quinn_impl::Certificate),
    #[cfg(feature = "inhouse")]
    Inhouse(inhouse_impl::Certificate),
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
#[derive(Clone)]
pub enum ListenerHandle {
    #[cfg(feature = "quinn")]
    Quinn(quinn_impl::Endpoint),
    #[cfg(feature = "s2n-quic")]
    S2n(Arc<s2n_impl::Server>),
    #[cfg(feature = "inhouse")]
    Inhouse(inhouse_impl::Endpoint),
}

impl ListenerHandle {
    #[cfg(feature = "quinn")]
    pub fn as_quinn(&self) -> Option<&quinn_impl::Endpoint> {
        match self {
            ListenerHandle::Quinn(endpoint) => Some(endpoint),
            #[cfg(feature = "s2n-quic")]
            ListenerHandle::S2n(_) => None,
            #[cfg(feature = "inhouse")]
            ListenerHandle::Inhouse(_) => None,
        }
    }

    #[cfg(feature = "quinn")]
    pub fn into_quinn(self) -> Option<quinn_impl::Endpoint> {
        match self {
            ListenerHandle::Quinn(endpoint) => Some(endpoint),
            #[cfg(feature = "s2n-quic")]
            ListenerHandle::S2n(_) => None,
            #[cfg(feature = "inhouse")]
            ListenerHandle::Inhouse(_) => None,
        }
    }

    #[cfg(feature = "s2n-quic")]
    pub fn as_s2n(&self) -> Option<&Arc<s2n_impl::Server>> {
        match self {
            #[cfg(feature = "quinn")]
            ListenerHandle::Quinn(_) => None,
            ListenerHandle::S2n(server) => Some(server),
            #[cfg(feature = "inhouse")]
            ListenerHandle::Inhouse(_) => None,
        }
    }

    #[cfg(feature = "s2n-quic")]
    pub fn into_s2n(self) -> Option<Arc<s2n_impl::Server>> {
        match self {
            #[cfg(feature = "quinn")]
            ListenerHandle::Quinn(_) => None,
            ListenerHandle::S2n(server) => Some(server),
            #[cfg(feature = "inhouse")]
            ListenerHandle::Inhouse(_) => None,
        }
    }

    #[cfg(feature = "inhouse")]
    pub fn as_inhouse(&self) -> Option<&inhouse_impl::Endpoint> {
        match self {
            #[cfg(feature = "quinn")]
            ListenerHandle::Quinn(_) => None,
            #[cfg(feature = "s2n-quic")]
            ListenerHandle::S2n(_) => None,
            ListenerHandle::Inhouse(endpoint) => Some(endpoint),
        }
    }

    #[cfg(feature = "inhouse")]
    pub fn into_inhouse(self) -> Option<inhouse_impl::Endpoint> {
        match self {
            #[cfg(feature = "quinn")]
            ListenerHandle::Quinn(_) => None,
            #[cfg(feature = "s2n-quic")]
            ListenerHandle::S2n(_) => None,
            ListenerHandle::Inhouse(endpoint) => Some(endpoint),
        }
    }
}

#[cfg(feature = "quinn")]
pub use quinn_impl::{
    classify_err, ConnectError, ConnectionStatsSnapshot, HandshakeError, QuinnDisconnect,
};

#[cfg(feature = "s2n-quic")]
pub use s2n_impl::{
    fingerprint, fingerprint_history, verify_remote_certificate, CertAdvertisement, LocalCert,
};

#[cfg(feature = "inhouse")]
pub use inhouse_impl::{
    certificate_store as inhouse_certificate_store, fingerprint as inhouse_fingerprint,
    fingerprint_history as inhouse_fingerprint_history,
    verify_remote_certificate as inhouse_verify_remote_certificate,
    Advertisement as InhouseAdvertisement,
    ConnectionStatsSnapshot as InhouseConnectionStatsSnapshot,
};

#[cfg(all(feature = "quinn", not(feature = "s2n-quic")))]
pub fn fingerprint_history() -> Vec<[u8; 32]> {
    Vec::new()
}

#[cfg(all(feature = "quinn", not(feature = "s2n-quic")))]
pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    let hash = blake3::hash(cert);
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    out
}

#[cfg(all(feature = "quinn", not(feature = "s2n-quic")))]
pub fn verify_remote_certificate(_peer_key: &[u8; 32], _cert: &[u8]) -> DiagResult<[u8; 32]> {
    Err(anyhow!("s2n-quic provider not compiled"))
}

#[cfg(feature = "s2n-quic")]
pub use s2n_impl::PROVIDER_ID as S2N_PROVIDER_ID;

#[cfg(feature = "quinn")]
pub use quinn_impl::PROVIDER_ID as QUINN_PROVIDER_ID;

#[cfg(feature = "inhouse")]
pub use inhouse_impl::PROVIDER_ID as INHOUSE_PROVIDER_ID;

pub fn provider_kind_from_id(id: &str) -> Option<ProviderKind> {
    match id {
        #[cfg(feature = "quinn")]
        x if x.eq_ignore_ascii_case(quinn_impl::PROVIDER_ID) => Some(ProviderKind::Quinn),
        #[cfg(feature = "s2n-quic")]
        x if x.eq_ignore_ascii_case(s2n_impl::PROVIDER_ID) => Some(ProviderKind::S2nQuic),
        #[cfg(feature = "inhouse")]
        x if x.eq_ignore_ascii_case(inhouse_impl::PROVIDER_ID) => Some(ProviderKind::Inhouse),
        _ => None,
    }
}

pub fn available_providers() -> Vec<ProviderMetadata> {
    #[allow(unused_mut)]
    let mut providers = Vec::new();
    #[cfg(feature = "quinn")]
    providers.push(ProviderMetadata {
        kind: ProviderKind::Quinn,
        id: quinn_impl::PROVIDER_ID,
        capabilities: quinn_impl::CAPABILITIES,
    });
    #[cfg(feature = "s2n-quic")]
    providers.push(ProviderMetadata {
        kind: ProviderKind::S2nQuic,
        id: s2n_impl::PROVIDER_ID,
        capabilities: s2n_impl::CAPABILITIES,
    });
    #[cfg(feature = "inhouse")]
    providers.push(ProviderMetadata {
        kind: ProviderKind::Inhouse,
        id: inhouse_impl::PROVIDER_ID,
        capabilities: inhouse_impl::CAPABILITIES,
    });
    providers
}

pub fn verify_remote_certificate_for(
    provider_id: &str,
    peer_key: &[u8; 32],
    cert: &[u8],
) -> DiagResult<[u8; 32]> {
    #[cfg(not(feature = "s2n-quic"))]
    let _ = (peer_key, cert);
    match provider_kind_from_id(provider_id) {
        #[cfg(feature = "s2n-quic")]
        Some(ProviderKind::S2nQuic) => Ok(s2n_impl::verify_remote_certificate(peer_key, cert)?),
        #[cfg(feature = "inhouse")]
        Some(ProviderKind::Inhouse) => Ok(inhouse_impl::verify_remote_certificate(peer_key, cert)?),
        #[cfg(feature = "quinn")]
        Some(ProviderKind::Quinn) => Err(anyhow!("quinn provider does not validate certificates")),
        _ => Err(anyhow!("unknown quic provider: {provider_id}")),
    }
}

pub fn fingerprint_history_for(provider_id: &str) -> Option<Vec<[u8; 32]>> {
    match provider_kind_from_id(provider_id) {
        #[cfg(feature = "s2n-quic")]
        Some(ProviderKind::S2nQuic) => Some(s2n_impl::fingerprint_history()),
        #[cfg(feature = "inhouse")]
        Some(ProviderKind::Inhouse) => Some(inhouse_impl::fingerprint_history()),
        _ => None,
    }
}

#[cfg(feature = "quinn")]
pub mod quinn_backend;

#[cfg(feature = "s2n-quic")]
pub mod s2n_backend;

#[cfg(feature = "inhouse")]
pub mod inhouse;
