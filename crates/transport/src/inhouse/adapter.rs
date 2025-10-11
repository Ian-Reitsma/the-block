use std::collections::{HashMap, VecDeque};
use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime};

use concurrency::Bytes;
use diagnostics::{anyhow, Result as DiagResult};
use foundation_tls::ed25519_public_key_from_der;
use rand::{rngs::OsRng, RngCore};
use runtime;
use runtime::net::UdpSocket;
use runtime::sync::{
    cancellation::CancellationToken,
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    mutex::Mutex as AsyncMutex,
};

use crate::{ProviderCapability, ProviderMetadata, RetryPolicy};

use super::certificate::{fingerprint, Certificate};
use super::messages::{
    decode_message, encode_application_ack, encode_application_data, encode_client_finish,
    encode_client_hello, encode_server_hello, Message, MessageError, MAX_DATAGRAM,
};
use super::store::InhouseCertificateStore;

const HANDSHAKE_ENTRY_TTL: Duration = Duration::from_secs(30);
const RETRANSMIT_INITIAL: Duration = Duration::from_millis(10);
const RETRANSMIT_MAX: Duration = Duration::from_millis(400);

pub const PROVIDER_ID: &str = "inhouse";

pub const CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::CertificateRotation,
    ProviderCapability::ConnectionPooling,
    ProviderCapability::TelemetryCallbacks,
];

#[derive(Clone, Default)]
pub struct InhouseEventCallbacks {
    pub handshake_success: Option<Arc<dyn Fn(SocketAddr) + Send + Sync + 'static>>,
    pub handshake_failure: Option<Arc<dyn Fn(SocketAddr, &str) + Send + Sync + 'static>>,
    pub provider_connect: Option<Arc<dyn Fn(&'static str) + Send + Sync + 'static>>,
}

#[derive(Clone)]
pub struct Adapter {
    inner: Arc<AdapterInner>,
}

struct AdapterInner {
    endpoints: Mutex<HashMap<SocketAddr, Weak<EndpointInner>>>,
    connections: Mutex<HashMap<SocketAddr, Arc<Connection>>>,
    callbacks: InhouseEventCallbacks,
    retry: RetryPolicy,
    handshake_timeout: Duration,
}

impl Adapter {
    pub fn new(
        retry: RetryPolicy,
        handshake_timeout: Duration,
        callbacks: &InhouseEventCallbacks,
    ) -> DiagResult<Self> {
        if let Some(hook) = callbacks.provider_connect.clone() {
            hook(PROVIDER_ID);
        }
        Ok(Self {
            inner: Arc::new(AdapterInner {
                endpoints: Mutex::new(HashMap::new()),
                connections: Mutex::new(HashMap::new()),
                callbacks: callbacks.clone(),
                retry,
                handshake_timeout,
            }),
        })
    }

    pub fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            kind: crate::ProviderKind::Inhouse,
            id: PROVIDER_ID,
            capabilities: CAPABILITIES,
        }
    }

    pub async fn listen(&self, addr: SocketAddr) -> DiagResult<(Endpoint, Certificate)> {
        {
            let mut endpoints = self.inner.endpoints.lock().unwrap();
            if let Some(existing) = endpoints.get(&addr) {
                if existing.upgrade().is_some() {
                    return Err(anyhow!("listener already active"));
                }
                endpoints.remove(&addr);
            }
        }

        let certificate = Certificate::generate()?;
        let socket = UdpSocket::bind(addr)
            .await
            .map_err(|err| anyhow!("bind udp socket: {err}"))?;
        let local_addr = socket
            .local_addr()
            .map_err(|err| anyhow!("inspect udp addr: {err}"))?;

        let table = Arc::new(Mutex::new(HandshakeTable::new(1024)));
        let shutdown = CancellationToken::new();
        let worker_shutdown = shutdown.clone();
        let worker_table = Arc::clone(&table);
        let worker_certificate = certificate.clone();
        let worker = runtime::spawn(async move {
            server_loop(worker_shutdown, worker_table, socket, worker_certificate).await;
        });

        // Allow the server loop to register with the runtime before returning so
        // immediate connection attempts do not race the listener startup.
        runtime::yield_now().await;

        let owner = Arc::downgrade(&self.inner);
        let endpoint = Arc::new(EndpointInner {
            owner,
            addr: local_addr,
            certificate: certificate.clone(),
            shutdown,
            _worker: worker,
        });

        self.inner
            .endpoints
            .lock()
            .unwrap()
            .insert(local_addr, Arc::downgrade(&endpoint));
        Ok((Endpoint { inner: endpoint }, certificate))
    }

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &Certificate,
    ) -> DiagResult<Arc<Connection>> {
        if let Some(conn) = self.inner.connections.lock().unwrap().get(&addr) {
            return Ok(conn.clone());
        }

        let mut attempts = 0usize;
        loop {
            attempts += 1;
            match attempt_handshake(addr, cert, self.inner.handshake_timeout).await {
                Ok(handshake) => {
                    let connection = Connection::from_handshake(handshake, cert.clone());
                    if let Some(cb) = &self.inner.callbacks.handshake_success {
                        cb(addr);
                    }
                    let mut connections = self.inner.connections.lock().unwrap();
                    connections.insert(addr, connection.clone());
                    return Ok(connection);
                }
                Err(err) => {
                    let err_label = err.to_string();
                    if attempts >= self.inner.retry.attempts {
                        if let Some(cb) = &self.inner.callbacks.handshake_failure {
                            cb(addr, &err_label);
                        }
                        return Err(anyhow!("handshake failed: {err_label}"));
                    }
                    runtime::sleep(self.inner.retry.backoff).await;
                }
            }
        }
    }

    pub async fn connect_insecure(&self, addr: SocketAddr) -> DiagResult<Arc<Connection>> {
        let cert = {
            let endpoints = self.inner.endpoints.lock().unwrap();
            endpoints
                .get(&addr)
                .and_then(|endpoint| endpoint.upgrade())
                .map(|endpoint| endpoint.certificate.clone())
                .ok_or_else(|| anyhow!("no listener"))?
        };
        self.connect(addr, &cert).await
    }

    pub fn drop_connection(&self, addr: &SocketAddr) {
        if let Some(conn) = self.inner.connections.lock().unwrap().remove(addr) {
            conn.close();
        }
    }

    pub fn connection_stats(&self) -> Vec<(SocketAddr, ConnectionStatsSnapshot)> {
        self.inner
            .connections
            .lock()
            .unwrap()
            .iter()
            .map(|(addr, conn)| (*addr, conn.stats()))
            .collect()
    }

    pub async fn send(&self, conn: &Arc<Connection>, data: &[u8]) -> DiagResult<()> {
        conn.send(data).await
    }

    pub async fn recv(&self, conn: &Arc<Connection>) -> Option<Vec<u8>> {
        conn.recv().await
    }

    pub fn verify_remote_certificate(
        &self,
        peer_key: &[u8; 32],
        cert: &[u8],
    ) -> DiagResult<[u8; 32]> {
        verify_remote_certificate(peer_key, cert)
    }

    pub fn certificate_from_der(&self, cert: Bytes) -> Certificate {
        Certificate::from_der_lossy(cert)
    }
}

#[derive(Clone)]
pub struct Endpoint {
    inner: Arc<EndpointInner>,
}

struct EndpointInner {
    owner: Weak<AdapterInner>,
    addr: SocketAddr,
    certificate: Certificate,
    shutdown: CancellationToken,
    _worker: runtime::JoinHandle<()>,
}

impl Endpoint {
    pub fn local_addr(&self) -> SocketAddr {
        self.inner.addr
    }
}

impl Drop for EndpointInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(owner) = self.owner.upgrade() {
            let mut endpoints = owner.endpoints.lock().unwrap();
            let self_ptr = self as *const EndpointInner as *mut EndpointInner;
            if let Some(entry) = endpoints.get(&self.addr) {
                let entry_ptr = Weak::as_ptr(entry);
                if entry_ptr == self_ptr || entry.upgrade().is_none() {
                    endpoints.remove(&self.addr);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Connection {
    inner: Arc<ConnectionInner>,
}

struct ConnectionInner {
    remote_addr: SocketAddr,
    handshake_id: [u8; 16],
    certificate: Certificate,
    socket: Arc<AsyncMutex<Option<UdpSocket>>>,
    incoming: AsyncMutex<UnboundedReceiver<Vec<u8>>>,
    shutdown: CancellationToken,
    deliveries: std::sync::atomic::AtomicU64,
    created_at: SystemTime,
}

impl Connection {
    fn from_handshake(handshake: ClientHandshake, certificate: Certificate) -> Arc<Self> {
        let socket = Arc::new(AsyncMutex::new(Some(handshake.socket)));
        let (tx, rx) = mpsc::unbounded_channel();
        let shutdown = CancellationToken::new();
        let inner = Arc::new(ConnectionInner {
            remote_addr: handshake.addr,
            handshake_id: handshake.handshake_id,
            certificate,
            socket: Arc::clone(&socket),
            incoming: AsyncMutex::new(rx),
            shutdown: shutdown.clone(),
            deliveries: std::sync::atomic::AtomicU64::new(0),
            created_at: SystemTime::now(),
        });
        let worker_inner = Arc::clone(&inner);
        runtime::spawn(async move {
            client_receiver_loop(worker_inner, socket, tx, shutdown).await;
        });
        Arc::new(Self { inner })
    }

    async fn send(&self, data: &[u8]) -> DiagResult<()> {
        let frame = encode_application_data(&self.inner.handshake_id, data);
        let mut socket = {
            let mut guard = self.inner.socket.lock().await;
            guard
                .take()
                .ok_or_else(|| anyhow!("connection socket closed"))?
        };
        let result = socket.send_to(&frame, self.inner.remote_addr).await;
        let mut guard = self.inner.socket.lock().await;
        *guard = Some(socket);
        result.map_err(|err| anyhow!("send datagram: {err}"))?;
        Ok(())
    }

    async fn recv(&self) -> Option<Vec<u8>> {
        let mut guard = self.inner.incoming.lock().await;
        guard.recv().await
    }

    fn stats(&self) -> ConnectionStatsSnapshot {
        ConnectionStatsSnapshot {
            established_at: self.inner.created_at,
            deliveries: self
                .inner
                .deliveries
                .load(std::sync::atomic::Ordering::SeqCst),
        }
    }

    fn close(&self) {
        self.inner.shutdown.cancel();
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.inner.remote_addr
    }

    pub fn certificate(&self) -> &Certificate {
        &self.inner.certificate
    }
}

impl Drop for ConnectionInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

#[derive(Clone, Copy)]
pub struct ConnectionStatsSnapshot {
    pub established_at: SystemTime,
    pub deliveries: u64,
}

pub fn certificate_store(path: std::path::PathBuf) -> InhouseCertificateStore {
    InhouseCertificateStore::new(path)
}

pub fn verify_remote_certificate(peer_key: &[u8; 32], cert: &[u8]) -> DiagResult<[u8; 32]> {
    if cert.is_empty() {
        return Err(anyhow!("certificate payload empty"));
    }
    let derived = ed25519_public_key_from_der(cert)
        .map_err(|err| anyhow!("certificate parse failed: {err}"))?;
    if &derived != peer_key {
        return Err(anyhow!("certificate public key mismatch"));
    }
    Ok(fingerprint(cert))
}

struct ClientHandshake {
    socket: UdpSocket,
    addr: SocketAddr,
    handshake_id: [u8; 16],
}

async fn attempt_handshake(
    addr: SocketAddr,
    cert: &Certificate,
    timeout_duration: Duration,
) -> DiagResult<ClientHandshake> {
    let local = match addr {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let mut socket = UdpSocket::bind(local)
        .await
        .map_err(|err| anyhow!("bind local udp socket: {err}"))?;

    let mut handshake_id = [0u8; 16];
    OsRng::default().fill_bytes(&mut handshake_id);
    let hello = encode_client_hello(&handshake_id);
    socket
        .send_to(&hello, addr)
        .await
        .map_err(|err| anyhow!("send client hello: {err}"))?;

    let mut schedule = RetransmitSchedule::new(timeout_duration);
    let deadline = Instant::now() + timeout_duration;
    let server_hello = loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| anyhow!("handshake timed out"))?;
        let window = schedule.current_window().min(remaining);
        match receive_server_hello(&mut socket, addr, &handshake_id, window).await? {
            ServerHelloWait::Received(hello) => break hello,
            ServerHelloWait::Timeout => {
                schedule.on_timeout();
                socket
                    .send_to(&hello, addr)
                    .await
                    .map_err(|err| anyhow!("resend client hello: {err}"))?;
            }
        }
    };

    if server_hello.fingerprint != cert.fingerprint {
        return Err(anyhow!("certificate fingerprint mismatch"));
    }
    let verifying_key = ed25519_public_key_from_der(&server_hello.certificate)
        .map_err(|err| anyhow!("certificate parse failed: {err}"))?;
    if verifying_key != cert.verifying_key {
        return Err(anyhow!("certificate public key mismatch"));
    }

    let finish = encode_client_finish(&handshake_id);
    socket
        .send_to(&finish, addr)
        .await
        .map_err(|err| anyhow!("send client finish: {err}"))?;

    Ok(ClientHandshake {
        socket,
        addr,
        handshake_id,
    })
}

struct ServerHello {
    fingerprint: [u8; 32],
    certificate: Vec<u8>,
}

async fn receive_server_hello(
    socket: &mut UdpSocket,
    expected_addr: SocketAddr,
    handshake: &[u8; 16],
    window: Duration,
) -> DiagResult<ServerHelloWait> {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let deadline = Instant::now() + window;
    loop {
        let now = Instant::now();
        let Some(remaining) = deadline.checked_duration_since(now) else {
            return Ok(ServerHelloWait::Timeout);
        };
        let recv = runtime::timeout(remaining, socket.recv_from(&mut buf)).await;
        let (len, peer) = match recv {
            Ok(Ok((len, peer))) => (len, peer),
            Ok(Err(err)) => return Err(anyhow!("recv server hello: {err}")),
            Err(_) => return Ok(ServerHelloWait::Timeout),
        };
        if peer != expected_addr {
            continue;
        }
        let message = match decode_message(&buf[..len]) {
            Ok(message) => message,
            Err(_) => continue,
        };
        if let Message::ServerHello {
            handshake: received,
            fingerprint,
            certificate,
        } = message
        {
            if received == *handshake {
                return Ok(ServerHelloWait::Received(ServerHello {
                    fingerprint,
                    certificate,
                }));
            }
        }
    }
}

async fn client_receiver_loop(
    connection: Arc<ConnectionInner>,
    socket: Arc<AsyncMutex<Option<UdpSocket>>>,
    tx: UnboundedSender<Vec<u8>>,
    shutdown: CancellationToken,
) {
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        match recv_datagram(&socket).await {
            Ok((payload, addr)) => {
                if addr != connection.remote_addr {
                    continue;
                }
                let message = match decode_message(&payload) {
                    Ok(message) => message,
                    Err(_) => continue,
                };
                if let Message::ApplicationAck { handshake, payload } = message {
                    if handshake == connection.handshake_id {
                        if tx.send(payload).is_ok() {
                            connection
                                .deliveries
                                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
}

async fn recv_datagram(
    socket: &Arc<AsyncMutex<Option<UdpSocket>>>,
) -> std::io::Result<(Vec<u8>, SocketAddr)> {
    let mut udp = {
        let mut guard = socket.lock().await;
        guard
            .take()
            .ok_or_else(|| IoError::new(ErrorKind::NotConnected, "connection socket closed"))?
    };
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let result = udp.recv_from(&mut buf).await;
    let mut guard = socket.lock().await;
    *guard = Some(udp);
    let (len, addr) = result?;
    buf.truncate(len);
    Ok((buf, addr))
}

async fn server_loop(
    shutdown: CancellationToken,
    table: Arc<Mutex<HandshakeTable>>,
    mut socket: UdpSocket,
    certificate: Certificate,
) {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        match socket.recv_from(&mut buf).await {
            Ok((len, peer)) => {
                let message = match decode_message(&buf[..len]) {
                    Ok(message) => message,
                    Err(MessageError::InvalidVersion(_)) | Err(MessageError::UnknownKind(_)) => {
                        continue;
                    }
                    Err(_) => continue,
                };
                match message {
                    Message::ClientHello { handshake } => {
                        let response = {
                            let mut guard = table.lock().unwrap();
                            guard.on_client_hello(handshake, peer, &certificate)
                        };
                        let _ = socket.send_to(&response, peer).await;
                    }
                    Message::ClientFinish { handshake } => {
                        let established = {
                            let mut guard = table.lock().unwrap();
                            guard.mark_established(handshake, peer)
                        };
                        if !established {
                            let response = {
                                let mut guard = table.lock().unwrap();
                                guard.on_client_hello(handshake, peer, &certificate)
                            };
                            let _ = socket.send_to(&response, peer).await;
                        }
                    }
                    Message::ApplicationData { handshake, payload } => {
                        let ack = {
                            let mut guard = table.lock().unwrap();
                            guard.ack_payload(&handshake, peer, &payload)
                        };
                        if let Some(ack) = ack {
                            let _ = socket.send_to(&ack, peer).await;
                        }
                    }
                    Message::ServerHello { .. } => {}
                    Message::ApplicationAck { .. } => {}
                }
            }
            Err(_) => break,
        }
    }
}

struct HandshakeTable {
    entries: HashMap<[u8; 16], HandshakeEntry>,
    order: VecDeque<[u8; 16]>,
    capacity: usize,
}

impl HandshakeTable {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn on_client_hello(
        &mut self,
        handshake: [u8; 16],
        addr: SocketAddr,
        certificate: &Certificate,
    ) -> Vec<u8> {
        self.prune_expired();
        if let Some(entry) = self.entries.get_mut(&handshake) {
            entry.addr = addr;
            entry.established = false;
            entry.refresh();
            return entry.server_hello.clone();
        }
        let response = encode_server_hello(&handshake, &certificate.fingerprint, &certificate.der);
        while self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
        self.order.push_back(handshake);
        self.entries.insert(
            handshake,
            HandshakeEntry::new(addr, response.clone(), false),
        );
        response
    }

    fn mark_established(&mut self, handshake: [u8; 16], addr: SocketAddr) -> bool {
        self.prune_expired();
        if let Some(entry) = self.entries.get_mut(&handshake) {
            if entry.addr == addr {
                entry.established = true;
                entry.refresh();
                return true;
            }
        }
        false
    }

    fn ack_payload(
        &mut self,
        handshake: &[u8; 16],
        addr: SocketAddr,
        payload: &[u8],
    ) -> Option<Vec<u8>> {
        self.prune_expired();
        let entry = self.entries.get_mut(handshake)?;
        if entry.addr != addr || !entry.established {
            return None;
        }
        entry.refresh();
        Some(encode_application_ack(handshake, payload))
    }

    fn prune_expired(&mut self) {
        let now = Instant::now();
        let mut retained = VecDeque::with_capacity(self.order.len());
        for handshake in self.order.drain(..) {
            let expired = match self.entries.get(&handshake) {
                Some(entry) => entry.expires_at <= now,
                None => true,
            };
            if expired {
                self.entries.remove(&handshake);
            } else {
                retained.push_back(handshake);
            }
        }
        self.order = retained;
    }
}

struct HandshakeEntry {
    addr: SocketAddr,
    expires_at: Instant,
    server_hello: Vec<u8>,
    established: bool,
}

impl HandshakeEntry {
    fn new(addr: SocketAddr, server_hello: Vec<u8>, established: bool) -> Self {
        let mut entry = Self {
            addr,
            expires_at: Instant::now(),
            server_hello,
            established,
        };
        entry.refresh();
        entry
    }

    fn refresh(&mut self) {
        self.expires_at = Instant::now() + HANDSHAKE_ENTRY_TTL;
    }
}

struct RetransmitSchedule {
    initial: Duration,
    current: Duration,
    max: Duration,
}

impl RetransmitSchedule {
    fn new(handshake_timeout: Duration) -> Self {
        let safe_timeout = handshake_timeout.max(Duration::from_millis(1));
        let initial = RETRANSMIT_INITIAL
            .min(safe_timeout)
            .max(Duration::from_millis(1));
        let max = RETRANSMIT_MAX.min(safe_timeout).max(initial);
        Self {
            initial,
            current: initial,
            max,
        }
    }

    fn current_window(&self) -> Duration {
        self.current
    }

    fn on_timeout(&mut self) {
        self.current = (self.current * 2).min(self.max).max(self.initial);
    }
}

enum ServerHelloWait {
    Received(ServerHello),
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_certificate() -> Certificate {
        Certificate {
            fingerprint: [5u8; 32],
            verifying_key: [7u8; 32],
            der: Bytes::from_static(b"dummy cert"),
        }
    }

    #[test]
    fn handshake_table_resends_cached_server_hello() {
        let mut table = HandshakeTable::new(4);
        let cert = sample_certificate();
        let handshake = [1u8; 16];
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7000);

        let first = table.on_client_hello(handshake, addr, &cert);
        assert!(!first.is_empty());

        // Duplicate hellos reuse the cached response and keep the entry alive.
        let duplicate = table.on_client_hello(handshake, addr, &cert);
        assert_eq!(first, duplicate);

        assert!(table.mark_established(handshake, addr));
        let ack = table.ack_payload(&handshake, addr, b"payload");
        assert!(ack.is_some());
    }

    #[test]
    fn handshake_table_expires_entries() {
        let mut table = HandshakeTable::new(4);
        let cert = sample_certificate();
        let handshake = [2u8; 16];
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7100);

        table.on_client_hello(handshake, addr, &cert);
        assert!(table.mark_established(handshake, addr));

        {
            let entry = table.entries.get_mut(&handshake).expect("entry");
            entry.expires_at = Instant::now() - Duration::from_secs(1);
        }

        table.prune_expired();
        assert!(table.ack_payload(&handshake, addr, b"payload").is_none());
    }

    #[test]
    fn retransmit_schedule_bounds_backoff() {
        let mut schedule = RetransmitSchedule::new(Duration::from_millis(120));
        let first = schedule.current_window();
        assert!(first >= Duration::from_millis(1));
        schedule.on_timeout();
        let second = schedule.current_window();
        assert!(second >= first);
        for _ in 0..5 {
            schedule.on_timeout();
        }
        assert!(schedule.current_window() <= RETRANSMIT_MAX);
    }
}
