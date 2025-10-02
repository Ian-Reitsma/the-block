use async_trait::async_trait;
use crypto_suite::hashing::blake3;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "s2n-quic")]
use crypto_suite::signatures::ed25519::SigningKey;

#[cfg(feature = "inhouse")]
use crate::inhouse_backend as inhouse_impl;

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
#[async_trait]
pub trait QuicListener {
    type Endpoint;
    type Error;

    async fn listen(&self, addr: SocketAddr) -> Result<Self::Endpoint, Self::Error>;
}

/// Generic QUIC connector interface implemented by backends.
#[async_trait]
pub trait QuicConnector {
    type Connection;
    type Error;

    async fn connect(&self, addr: SocketAddr) -> Result<Self::Connection, Self::Error>;
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

/// Common configuration for the transport abstraction.
#[derive(Clone, Debug)]
pub struct Config {
    pub provider: ProviderKind,
    pub certificate_cache: Option<PathBuf>,
    pub retry: RetryPolicy,
    pub handshake_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Quinn,
            certificate_cache: None,
            retry: RetryPolicy::default(),
            handshake_timeout: Duration::from_secs(5),
        }
    }
}

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
use anyhow::{anyhow, Result as AnyResult};

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
    fn create(&self, cfg: &Config, callbacks: &TransportCallbacks) -> AnyResult<ProviderRegistry>;
}

/// Default factory wiring the concrete backend implementations.
#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
#[derive(Clone, Default)]
pub struct DefaultFactory;

#[cfg(any(feature = "quinn", feature = "s2n-quic", feature = "inhouse"))]
impl TransportFactory for DefaultFactory {
    fn create(&self, cfg: &Config, callbacks: &TransportCallbacks) -> AnyResult<ProviderRegistry> {
        let instance = match cfg.provider {
            ProviderKind::Quinn => {
                #[cfg(feature = "quinn")]
                {
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
                    ProviderInstance::new_s2n(cfg, &callbacks.s2n)?
                }
                #[cfg(not(feature = "s2n-quic"))]
                {
                    return Err(anyhow!("s2n-quic provider not compiled"));
                }
            }
            #[cfg(feature = "inhouse")]
            ProviderKind::Inhouse => ProviderInstance::new_inhouse(cfg, &callbacks.inhouse)?,
        };
        Ok(ProviderRegistry {
            inner: Arc::new(instance),
        })
    }
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
        if let ProviderInstance::S2n(adapter) = self.inner.as_ref() {
            Some(adapter.clone())
        } else {
            None
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
    fn new_quinn(cfg: &Config, callbacks: &QuinnCallbacks) -> AnyResult<Self> {
        QuinnAdapter::new(cfg, callbacks).map(Self::Quinn)
    }

    #[cfg(feature = "s2n-quic")]
    fn new_s2n(cfg: &Config, callbacks: &S2nCallbacks) -> AnyResult<Self> {
        S2nAdapter::new(cfg, callbacks).map(Self::S2n)
    }

    #[cfg(feature = "inhouse")]
    fn new_inhouse(cfg: &Config, callbacks: &InhouseCallbacks) -> AnyResult<Self> {
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
    fn new(cfg: &Config, callbacks: &QuinnCallbacks) -> AnyResult<Self> {
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

    pub async fn listen(&self, addr: SocketAddr) -> AnyResult<(ListenerHandle, CertificateHandle)> {
        let (endpoint, cert) = quinn_impl::listen(addr).await?;
        Ok((
            ListenerHandle::Quinn(endpoint),
            CertificateHandle::Quinn(cert),
        ))
    }

    pub async fn listen_with_cert(
        &self,
        addr: SocketAddr,
        cert_der: &[u8],
        key_der: &[u8],
    ) -> AnyResult<ListenerHandle> {
        let endpoint = quinn_impl::listen_with_cert(addr, cert_der, key_der).await?;
        Ok(ListenerHandle::Quinn(endpoint))
    }

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &CertificateHandle,
    ) -> Result<ConnectionHandle, quinn_impl::ConnectError> {
        let cert = match cert {
            CertificateHandle::Quinn(cert) => cert.clone(),
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
            CertificateHandle::Quinn(cert) => cert.clone(),
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

    pub async fn send(&self, conn: &ConnectionHandle, data: &[u8]) -> AnyResult<()> {
        match conn {
            ConnectionHandle::Quinn(conn) => quinn_impl::send(conn, data).await.map_err(Into::into),
        }
    }

    pub async fn recv(&self, conn: &ConnectionHandle) -> Option<Vec<u8>> {
        match conn {
            ConnectionHandle::Quinn(conn) => quinn_impl::recv(conn).await,
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub async fn connect_insecure(
        &self,
        addr: SocketAddr,
    ) -> Result<ConnectionHandle, quinn_impl::ConnectError> {
        let conn = quinn_impl::connect_insecure(addr).await?;
        Ok(ConnectionHandle::Quinn(conn))
    }

    pub fn certificate_from_der(&self, cert: Vec<u8>) -> CertificateHandle {
        CertificateHandle::Quinn(quinn_impl::Certificate(cert))
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
    fn new(cfg: &Config, callbacks: &InhouseCallbacks) -> AnyResult<Self> {
        let mut backend_callbacks = inhouse_impl::InhouseEventCallbacks::default();
        backend_callbacks.handshake_success = callbacks.handshake_success.clone();
        backend_callbacks.handshake_failure = callbacks.handshake_failure.clone();
        backend_callbacks.provider_connect = callbacks.provider_connect.clone();
        let adapter = inhouse_impl::Adapter::new(cfg.retry.clone(), &backend_callbacks)?;
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

    pub async fn listen(&self, addr: SocketAddr) -> AnyResult<(ListenerHandle, CertificateHandle)> {
        let (endpoint, cert) = self.0.backend.listen(addr).await?;
        Ok((
            ListenerHandle::Inhouse(endpoint),
            CertificateHandle::Inhouse(cert),
        ))
    }

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &CertificateHandle,
    ) -> AnyResult<ConnectionHandle> {
        let cert = match cert {
            CertificateHandle::Inhouse(cert) => cert,
            #[cfg(feature = "quinn")]
            CertificateHandle::Quinn(_) => {
                return Err(anyhow!("certificate incompatible with inhouse provider"));
            }
        };
        let conn = self.0.backend.connect(addr, cert).await?;
        Ok(ConnectionHandle::Inhouse(conn))
    }

    pub async fn connect_insecure(&self, addr: SocketAddr) -> AnyResult<ConnectionHandle> {
        let conn = self.0.backend.connect_insecure(addr).await?;
        Ok(ConnectionHandle::Inhouse(conn))
    }

    pub fn drop_connection(&self, addr: &SocketAddr) {
        self.0.backend.drop_connection(addr);
    }

    pub fn connection_stats(&self) -> Vec<(SocketAddr, inhouse_impl::ConnectionStatsSnapshot)> {
        self.0.backend.connection_stats()
    }

    pub async fn send(&self, conn: &ConnectionHandle, data: &[u8]) -> AnyResult<()> {
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
    ) -> AnyResult<[u8; 32]> {
        Ok(self.0.backend.verify_remote_certificate(peer_key, cert)?)
    }

    pub fn certificate_from_der(&self, cert: Vec<u8>) -> CertificateHandle {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&cert);
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(hasher.finalize().as_bytes());
        CertificateHandle::Inhouse(inhouse_impl::Certificate {
            fingerprint,
            der: cert,
        })
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
    fn new(cfg: &Config, callbacks: &S2nCallbacks) -> AnyResult<Self> {
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

    pub fn initialize(
        &self,
        signing_key: &SigningKey,
    ) -> anyhow::Result<s2n_impl::CertAdvertisement> {
        s2n_impl::initialize(signing_key)
    }

    pub fn rotate(&self, signing_key: &SigningKey) -> anyhow::Result<s2n_impl::CertAdvertisement> {
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
    ) -> anyhow::Result<[u8; 32]> {
        s2n_impl::verify_remote_certificate(peer_key, cert)
    }

    pub async fn start_server(
        &self,
        addr: SocketAddr,
        signing_key: &SigningKey,
    ) -> Result<ListenerHandle, Box<dyn std::error::Error>> {
        let server = s2n_impl::start_server(addr, signing_key).await?;
        Ok(ListenerHandle::S2n(server))
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
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
    Quinn(quinn::Connection),
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
    Quinn(quinn::Endpoint),
    #[cfg(feature = "s2n-quic")]
    S2n(Arc<s2n_impl::Server>),
    #[cfg(feature = "inhouse")]
    Inhouse(inhouse_impl::Endpoint),
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
pub fn verify_remote_certificate(_peer_key: &[u8; 32], _cert: &[u8]) -> AnyResult<[u8; 32]> {
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
) -> AnyResult<[u8; 32]> {
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
pub mod inhouse_backend;
