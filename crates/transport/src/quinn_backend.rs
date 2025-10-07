use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use diagnostics::{anyhow, Result, TbError};
use once_cell::sync::{Lazy, OnceCell};
pub use quinn::{Connection, Endpoint};
use rand::Rng;
use rcgen::generate_simple_self_signed;
use rustls::client::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier, WebPkiVerifier,
};
pub use rustls::Certificate;
use rustls::{ClientConfig, PrivateKey, RootCertStore};
use rustls::{DigitallySignedStruct, ServerName, SignatureScheme};
use std::sync::RwLock;

use crate::{ProviderCapability, RetryPolicy};

static CONNECTIONS: Lazy<DashMap<SocketAddr, Connection>> = Lazy::new(|| DashMap::new());
static CALLBACKS: OnceCell<RwLock<Arc<QuinnEventCallbacks>>> = OnceCell::new();
static RETRY_POLICY: Lazy<RwLock<RetryPolicy>> = Lazy::new(|| RwLock::new(RetryPolicy::default()));
static HANDSHAKE_TIMEOUT: Lazy<RwLock<Duration>> =
    Lazy::new(|| RwLock::new(Duration::from_secs(5)));

pub fn set_retry_policy(policy: RetryPolicy) {
    *RETRY_POLICY.write().unwrap() = policy;
}

fn retry_policy() -> RetryPolicy {
    RETRY_POLICY.read().unwrap().clone()
}

pub fn set_handshake_timeout(timeout: Duration) {
    *HANDSHAKE_TIMEOUT.write().unwrap() = timeout;
}

fn handshake_timeout() -> Duration {
    *HANDSHAKE_TIMEOUT.read().unwrap()
}

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

pub fn set_event_callbacks(
    callbacks: QuinnEventCallbacks,
) -> std::result::Result<(), QuinnCallbackError> {
    let cell = CALLBACKS.get_or_init(|| RwLock::new(Arc::new(QuinnEventCallbacks::default())));
    let mut guard = cell.write().unwrap();
    *guard = Arc::new(callbacks);
    Ok(())
}

fn with_callbacks<F>(f: F)
where
    F: FnOnce(&QuinnEventCallbacks),
{
    if let Some(cell) = CALLBACKS.get() {
        let callbacks = cell.read().unwrap().clone();
        f(callbacks.as_ref());
    }
}

#[derive(Debug)]
pub enum ConnectError {
    Handshake(HandshakeError),
    Other(TbError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Handshake(e) => write!(f, "handshake failed: {}", e.as_str()),
            Self::Other(e) => write!(f, "{e}"),
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

pub async fn listen(addr: SocketAddr) -> Result<(Endpoint, Certificate)> {
    let cert = generate_simple_self_signed(["the-block".to_string()]).map_err(|e| anyhow!(e))?;
    let cert_der = cert.serialize_der().map_err(|e| anyhow!(e))?;
    let key_der = cert.serialize_private_key_der();
    let cert = Certificate(cert_der.clone());
    let key = PrivateKey(key_der);
    let server_config =
        quinn::ServerConfig::with_single_cert(vec![cert.clone()], key).map_err(|e| anyhow!(e))?;
    let policy = retry_policy();
    let mut attempts = 0usize;
    loop {
        match Endpoint::server(server_config.clone(), addr) {
            Ok(endpoint) => return Ok((endpoint, cert)),
            Err(_e) if attempts < policy.attempts => {
                attempts += 1;
                runtime::sleep(policy.backoff).await;
                continue;
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }
}

pub async fn listen_with_cert(
    addr: SocketAddr,
    cert_der: &[u8],
    key_der: &[u8],
) -> Result<Endpoint> {
    let cert = Certificate(cert_der.to_vec());
    let key = PrivateKey(key_der.to_vec());
    let server_config =
        quinn::ServerConfig::with_single_cert(vec![cert], key).map_err(|e| anyhow!(e))?;
    let policy = retry_policy();
    let mut attempts = 0usize;
    loop {
        match Endpoint::server(server_config.clone(), addr) {
            Ok(endpoint) => return Ok(endpoint),
            Err(_e) if attempts < policy.attempts => {
                attempts += 1;
                runtime::sleep(policy.backoff).await;
                continue;
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }
}

pub async fn connect(
    addr: SocketAddr,
    cert: Certificate,
) -> std::result::Result<Connection, ConnectError> {
    let mut roots = RootCertStore::empty();
    roots
        .add(&cert)
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let crypto = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let start = Instant::now();
    let attempt = endpoint
        .connect_with(client_cfg, addr, "the-block")
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let res = runtime::timeout(handshake_timeout(), attempt).await;
    match res {
        Ok(Ok(conn)) => {
            let elapsed = start.elapsed();
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_latency.as_ref() {
                    cb(addr, elapsed);
                }
                if let Some(cb) = cbs.provider_connect.as_ref() {
                    cb(PROVIDER_ID);
                }
            });
            Ok(conn)
        }
        Ok(Err(e)) => {
            let err = classify_err(&e);
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_failure.as_ref() {
                    cb(addr, err);
                }
            });
            Err(ConnectError::Handshake(err))
        }
        Err(_) => {
            let err = HandshakeError::Timeout;
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_failure.as_ref() {
                    cb(addr, err);
                }
            });
            Err(ConnectError::Handshake(err))
        }
    }
}

pub async fn get_connection(
    addr: SocketAddr,
    cert: Certificate,
) -> std::result::Result<Connection, ConnectError> {
    if let Some(existing) = CONNECTIONS.get(&addr) {
        if existing.close_reason().is_none() {
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.endpoint_reuse.as_ref() {
                    cb(addr);
                }
            });
            return Ok(existing.clone());
        } else {
            CONNECTIONS.remove(&addr);
        }
    }
    let conn = connect(addr, cert).await?;
    CONNECTIONS.insert(addr, conn.clone());
    Ok(conn)
}

pub fn drop_connection(addr: &SocketAddr) {
    CONNECTIONS.remove(addr);
}

pub fn connection_stats() -> Vec<(SocketAddr, ConnectionStatsSnapshot)> {
    CONNECTIONS
        .iter()
        .map(|entry| {
            let stats = entry.value().stats();
            (
                *entry.key(),
                ConnectionStatsSnapshot {
                    lost_packets: stats.path.lost_packets,
                    rtt: stats.path.rtt,
                },
            )
        })
        .collect()
}

pub async fn send(conn: &Connection, data: &[u8]) -> Result<()> {
    let mut rng = rand::thread_rng();
    if let Ok(loss_str) = std::env::var("TB_QUIC_PACKET_LOSS") {
        if let Ok(loss) = loss_str.parse::<f64>() {
            if rng.gen_bool(loss) {
                return Ok(());
            }
        }
    }
    let mut stream = match conn.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            notify_conn_err(conn.remote_address(), &e);
            return Err(anyhow!(e));
        }
    };
    if let Err(e) = stream.write_all(data).await {
        notify_write_err(conn.remote_address(), &e);
        return Err(anyhow!(e));
    }
    if let Ok(dup_str) = std::env::var("TB_QUIC_PACKET_DUP") {
        if let Ok(dup) = dup_str.parse::<f64>() {
            if rng.gen_bool(dup) {
                let _ = stream.write_all(data).await;
            }
        }
    }
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.bytes_sent.as_ref() {
            cb(conn.remote_address(), data.len() as u64);
        }
    });
    if let Err(e) = stream.finish().await {
        notify_write_err(conn.remote_address(), &e);
        return Err(anyhow!(e));
    }
    Ok(())
}

pub async fn recv(conn: &Connection) -> Option<Vec<u8>> {
    match conn.accept_uni().await {
        Ok(mut s) => match s.read_to_end(usize::MAX).await {
            Ok(buf) => {
                with_callbacks(|cbs| {
                    if let Some(cb) = cbs.bytes_received.as_ref() {
                        cb(conn.remote_address(), buf.len() as u64);
                    }
                });
                Some(buf)
            }
            Err(e) => {
                notify_read_err(conn.remote_address(), &e);
                None
            }
        },
        Err(e) => {
            notify_conn_err(conn.remote_address(), &e);
            None
        }
    }
}

pub fn classify_err(e: &quinn::ConnectionError) -> HandshakeError {
    match e {
        quinn::ConnectionError::TimedOut => return HandshakeError::Timeout,
        quinn::ConnectionError::VersionMismatch => return HandshakeError::Version,
        _ => {}
    }
    let msg = e.to_string().to_lowercase();
    if msg.contains("certificate") {
        HandshakeError::Certificate
    } else if msg.contains("tls") {
        HandshakeError::Tls
    } else {
        HandshakeError::Other
    }
}

fn notify_conn_err(addr: SocketAddr, e: &quinn::ConnectionError) {
    notify_disconnect(addr, map_conn_err(e));
}

fn notify_read_err(addr: SocketAddr, err: &quinn::ReadToEndError) {
    match err {
        quinn::ReadToEndError::Read(read) => notify_disconnect(addr, map_read_err(read)),
        quinn::ReadToEndError::TooLong => notify_disconnect(addr, QuinnDisconnect::WriteFailure),
    }
}

fn notify_disconnect(addr: SocketAddr, disconnect: QuinnDisconnect) {
    with_callbacks(|cbs| {
        if let Some(cb) = cbs.disconnect.as_ref() {
            cb(addr, disconnect);
        }
    });
}

fn notify_write_err(addr: SocketAddr, e: &quinn::WriteError) {
    notify_disconnect(addr, map_write_err(e));
}

fn map_conn_err(e: &quinn::ConnectionError) -> QuinnDisconnect {
    match e {
        quinn::ConnectionError::ApplicationClosed(ac) => QuinnDisconnect::ApplicationClosed {
            code: ac.error_code.into(),
        },
        quinn::ConnectionError::ConnectionClosed(cc) => QuinnDisconnect::ConnectionClosed {
            code: cc.error_code.into(),
        },
        quinn::ConnectionError::Reset => QuinnDisconnect::Reset,
        quinn::ConnectionError::TransportError(te) => QuinnDisconnect::TransportError {
            code: te.code.into(),
        },
        _ => QuinnDisconnect::WriteFailure,
    }
}

fn map_read_err(e: &quinn::ReadError) -> QuinnDisconnect {
    match e {
        quinn::ReadError::Reset(code) => QuinnDisconnect::ApplicationClosed {
            code: (*code).into(),
        },
        quinn::ReadError::ConnectionLost(conn) => map_conn_err(conn),
        quinn::ReadError::UnknownStream | quinn::ReadError::IllegalOrderedRead => {
            QuinnDisconnect::WriteFailure
        }
        quinn::ReadError::ZeroRttRejected => QuinnDisconnect::TransportError { code: 0 },
    }
}

fn map_write_err(e: &quinn::WriteError) -> QuinnDisconnect {
    match e {
        quinn::WriteError::ConnectionLost(conn) => map_conn_err(conn),
        quinn::WriteError::Stopped(code) => QuinnDisconnect::WriteStopped {
            code: (*code).into(),
        },
        _ => QuinnDisconnect::WriteFailure,
    }
}

#[cfg(any(test, debug_assertions))]
pub async fn connect_insecure(addr: SocketAddr) -> std::result::Result<Connection, ConnectError> {
    struct SkipCertVerification;
    impl ServerCertVerifier for SkipCertVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &Certificate,
            _intermediates: &[Certificate],
            _server_name: &ServerName,
            _scts: &mut dyn Iterator<Item = &[u8]>,
            _ocsp_response: &[u8],
            _now: std::time::SystemTime,
        ) -> std::result::Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &Certificate,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &Certificate,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            WebPkiVerifier::new(RootCertStore::empty(), None).supported_verify_schemes()
        }
    }
    let verifier = Arc::new(SkipCertVerification);
    let crypto = ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let start = Instant::now();
    let attempt = endpoint
        .connect_with(client_cfg, addr, "the-block")
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let res = runtime::timeout(std::time::Duration::from_secs(5), attempt).await;
    match res {
        Ok(Ok(conn)) => {
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_latency.as_ref() {
                    cb(addr, start.elapsed());
                }
            });
            Ok(conn)
        }
        Ok(Err(e)) => {
            let err = classify_err(&e);
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_failure.as_ref() {
                    cb(addr, err);
                }
            });
            Err(ConnectError::Handshake(err))
        }
        Err(_) => {
            let err = HandshakeError::Timeout;
            with_callbacks(|cbs| {
                if let Some(cb) = cbs.handshake_failure.as_ref() {
                    cb(addr, err);
                }
            });
            Err(ConnectError::Handshake(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn classify_err_variants() {
        assert_eq!(
            classify_err(&quinn::ConnectionError::VersionMismatch),
            HandshakeError::Version
        );
        assert_eq!(
            classify_err(&quinn::ConnectionError::TimedOut),
            HandshakeError::Timeout
        );
    }

    #[test]
    fn listen_retries_until_port_is_available() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind retry guard");
        let addr = socket.local_addr().expect("socket addr");
        let join = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(socket);
        });

        runtime::block_on(async move {
            set_retry_policy(RetryPolicy {
                attempts: 5,
                backoff: Duration::from_millis(5),
            });

            let (endpoint, _cert) = listen(addr)
                .await
                .expect("listener should retry until free");
            endpoint.close(0u32.into(), b"test");
            let _ = endpoint.wait_idle().await;
            set_retry_policy(RetryPolicy::default());
        });

        join.join().expect("retry release thread");
    }

    #[test]
    fn replacing_callbacks_updates_handlers() {
        let counter = Arc::new(AtomicUsize::new(0));
        let first = counter.clone();
        set_event_callbacks({
            let mut callbacks = QuinnEventCallbacks::default();
            callbacks.endpoint_reuse = Some(Arc::new(move |_addr| {
                first.fetch_add(1, Ordering::SeqCst);
            }));
            callbacks
        })
        .expect("install callbacks");

        super::with_callbacks(|callbacks| {
            if let Some(handler) = callbacks.endpoint_reuse.as_ref() {
                handler("127.0.0.1:7000".parse().unwrap());
            }
        });

        let second = counter.clone();
        set_event_callbacks({
            let mut callbacks = QuinnEventCallbacks::default();
            callbacks.endpoint_reuse = Some(Arc::new(move |_addr| {
                second.fetch_add(10, Ordering::SeqCst);
            }));
            callbacks
        })
        .expect("replace callbacks");

        super::with_callbacks(|callbacks| {
            if let Some(handler) = callbacks.endpoint_reuse.as_ref() {
                handler("127.0.0.1:7001".parse().unwrap());
            }
        });

        assert_eq!(counter.load(Ordering::SeqCst), 11);

        let _ = set_event_callbacks(QuinnEventCallbacks::default());
    }
}
