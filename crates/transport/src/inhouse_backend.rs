use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{anyhow, Context, Result};
use crypto_suite::hashing::blake3::{hash, Hasher};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{CertificateStore, ProviderCapability, ProviderMetadata, RetryPolicy};

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
    endpoints: Mutex<HashMap<SocketAddr, EndpointState>>,
    connections: Mutex<HashMap<SocketAddr, Arc<Connection>>>,
    callbacks: InhouseEventCallbacks,
    retry: RetryPolicy,
}

impl Adapter {
    pub fn new(retry: RetryPolicy, callbacks: &InhouseEventCallbacks) -> Result<Self> {
        if let Some(hook) = callbacks.provider_connect.clone() {
            hook(PROVIDER_ID);
        }
        Ok(Self {
            inner: Arc::new(AdapterInner {
                endpoints: Mutex::new(HashMap::new()),
                connections: Mutex::new(HashMap::new()),
                callbacks: callbacks.clone(),
                retry,
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

    pub async fn listen(&self, addr: SocketAddr) -> Result<(Endpoint, Certificate)> {
        let mut endpoints = self.inner.endpoints.lock().unwrap();
        if endpoints.contains_key(&addr) {
            return Err(anyhow!("listener already active"));
        }

        let certificate = Certificate::generate()?;
        endpoints.insert(
            addr,
            EndpointState {
                certificate: certificate.clone(),
            },
        );
        Ok((Endpoint { addr }, certificate))
    }

    pub async fn connect(&self, addr: SocketAddr, cert: &Certificate) -> Result<Arc<Connection>> {
        let mut attempts = 0usize;
        loop {
            attempts += 1;
            match self.try_connect(addr, cert) {
                Ok(conn) => return Ok(conn),
                Err(err) => {
                    if attempts >= self.inner.retry.attempts {
                        let label = err.to_string();
                        if let Some(cb) = &self.inner.callbacks.handshake_failure {
                            cb(addr, label.as_str());
                        }
                        return Err(anyhow!("handshake failed: {label}"));
                    }
                    runtime::sleep(self.inner.retry.backoff).await;
                }
            }
        }
    }

    pub async fn connect_insecure(&self, addr: SocketAddr) -> Result<Arc<Connection>> {
        let cert = {
            let endpoints = self.inner.endpoints.lock().unwrap();
            endpoints
                .get(&addr)
                .map(|state| state.certificate.clone())
                .ok_or_else(|| anyhow!("no listener"))?
        };
        self.connect(addr, &cert).await
    }

    pub fn drop_connection(&self, addr: &SocketAddr) {
        self.inner.connections.lock().unwrap().remove(addr);
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

    pub async fn send(&self, conn: &Arc<Connection>, data: &[u8]) -> Result<()> {
        conn.enqueue(data);
        Ok(())
    }

    pub async fn recv(&self, conn: &Arc<Connection>) -> Option<Vec<u8>> {
        conn.dequeue()
    }

    pub fn verify_remote_certificate(&self, peer_key: &[u8; 32], cert: &[u8]) -> Result<[u8; 32]> {
        verify_remote_certificate(peer_key, cert)
    }

    fn try_connect(&self, addr: SocketAddr, cert: &Certificate) -> Result<Arc<Connection>> {
        let endpoints = self.inner.endpoints.lock().unwrap();
        let state = endpoints
            .get(&addr)
            .ok_or_else(|| anyhow!("no listener registered"))?;
        if state.certificate.fingerprint != cert.fingerprint {
            return Err(anyhow!("certificate mismatch"));
        }
        drop(endpoints);

        let mut connections = self.inner.connections.lock().unwrap();
        if let Some(conn) = connections.get(&addr) {
            return Ok(conn.clone());
        }

        let conn = Arc::new(Connection::new(addr, cert.clone()));
        if let Some(cb) = &self.inner.callbacks.handshake_success {
            cb(addr);
        }
        connections.insert(addr, conn.clone());
        Ok(conn)
    }
}

#[derive(Clone)]
pub struct Endpoint {
    addr: SocketAddr,
}

impl Endpoint {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub fingerprint: [u8; 32],
    pub der: Vec<u8>,
}

impl Certificate {
    fn generate() -> Result<Self> {
        let mut random = [0u8; 64];
        OsRng.fill_bytes(&mut random);
        let mut hasher = Hasher::new();
        hasher.update(&random);
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(hasher.finalize().as_bytes());
        Ok(Self {
            fingerprint,
            der: random.to_vec(),
        })
    }
}

#[derive(Clone)]
pub struct Connection {
    addr: SocketAddr,
    certificate: Certificate,
    queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    created_at: SystemTime,
    deliveries: Arc<Mutex<u64>>,
}

impl Connection {
    fn new(addr: SocketAddr, certificate: Certificate) -> Self {
        Self {
            addr,
            certificate,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            created_at: SystemTime::now(),
            deliveries: Arc::new(Mutex::new(0)),
        }
    }

    fn enqueue(&self, data: &[u8]) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(data.to_vec());
        *self.deliveries.lock().unwrap() += 1;
    }

    fn dequeue(&self) -> Option<Vec<u8>> {
        self.queue.lock().unwrap().pop_front()
    }

    fn stats(&self) -> ConnectionStatsSnapshot {
        ConnectionStatsSnapshot {
            established_at: self.created_at,
            deliveries: *self.deliveries.lock().unwrap(),
        }
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn certificate(&self) -> &Certificate {
        &self.certificate
    }
}

#[derive(Clone, Copy)]
pub struct ConnectionStatsSnapshot {
    pub established_at: SystemTime,
    pub deliveries: u64,
}

struct EndpointState {
    certificate: Certificate,
}

#[derive(Clone)]
pub struct InhouseCertificateStore {
    path: PathBuf,
    current: Arc<Mutex<Option<Advertisement>>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Advertisement {
    pub fingerprint: [u8; 32],
    pub issued_at: SystemTime,
}

impl InhouseCertificateStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            current: Arc::new(Mutex::new(None)),
        }
    }

    fn persist(&self, advert: &Advertisement) -> Result<()> {
        let mut file = File::create(&self.path).with_context(|| "create cert store")?;
        let json = serde_json::to_vec(advert)?;
        file.write_all(&json)?;
        file.sync_all()?;
        Ok(())
    }

    fn load(&self) -> Result<Option<Advertisement>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let mut file = File::open(&self.path).with_context(|| "open cert store")?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if buf.is_empty() {
            return Ok(None);
        }
        let advert: Advertisement = serde_json::from_slice(&buf)?;
        Ok(Some(advert))
    }

    fn generate(&self) -> Result<Advertisement> {
        let certificate = Certificate::generate()?;
        Ok(Advertisement {
            fingerprint: certificate.fingerprint,
            issued_at: SystemTime::now(),
        })
    }
}

impl CertificateStore for InhouseCertificateStore {
    type Advertisement = Advertisement;
    type Error = anyhow::Error;

    fn initialize(&self) -> Result<Self::Advertisement, Self::Error> {
        let advert = self.generate()?;
        self.persist(&advert)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    fn rotate(&self) -> Result<Self::Advertisement, Self::Error> {
        let advert = self.generate()?;
        self.persist(&advert)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    fn current(&self) -> Option<Self::Advertisement> {
        if let Some(current) = self.current.lock().unwrap().clone() {
            return Some(current);
        }
        match self.load() {
            Ok(Some(advert)) => {
                *self.current.lock().unwrap() = Some(advert.clone());
                Some(advert)
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }
}

pub fn certificate_store(path: PathBuf) -> InhouseCertificateStore {
    // Ensure parent exists for deterministic behaviour in tests.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    InhouseCertificateStore::new(path)
}

pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    let digest = hash(cert);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_bytes());
    out
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    Vec::new()
}

pub fn verify_remote_certificate(_peer_key: &[u8; 32], cert: &[u8]) -> Result<[u8; 32]> {
    if cert.is_empty() {
        return Err(anyhow!("certificate payload empty"));
    }
    Ok(fingerprint(cert))
}
