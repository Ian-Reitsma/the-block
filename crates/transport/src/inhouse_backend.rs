use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::result::Result as StdResult;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crypto_suite::hashing::blake3::{hash, Hasher};
use diagnostics::{anyhow, Result as DiagResult, TbError};
use foundation_serialization::json::{self, Map, Value};
use rand::{rngs::OsRng, RngCore};

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
    pub fn new(retry: RetryPolicy, callbacks: &InhouseEventCallbacks) -> DiagResult<Self> {
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

    pub async fn listen(&self, addr: SocketAddr) -> DiagResult<(Endpoint, Certificate)> {
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

    pub async fn connect(
        &self,
        addr: SocketAddr,
        cert: &Certificate,
    ) -> DiagResult<Arc<Connection>> {
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

    pub async fn connect_insecure(&self, addr: SocketAddr) -> DiagResult<Arc<Connection>> {
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

    pub async fn send(&self, conn: &Arc<Connection>, data: &[u8]) -> DiagResult<()> {
        conn.enqueue(data);
        Ok(())
    }

    pub async fn recv(&self, conn: &Arc<Connection>) -> Option<Vec<u8>> {
        conn.dequeue()
    }

    pub fn verify_remote_certificate(
        &self,
        peer_key: &[u8; 32],
        cert: &[u8],
    ) -> DiagResult<[u8; 32]> {
        verify_remote_certificate(peer_key, cert)
    }

    fn try_connect(&self, addr: SocketAddr, cert: &Certificate) -> DiagResult<Arc<Connection>> {
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
    fn generate() -> DiagResult<Self> {
        let mut random = [0u8; 64];
        OsRng::default().fill_bytes(&mut random);
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

#[derive(Clone)]
pub struct Advertisement {
    pub fingerprint: [u8; 32],
    pub issued_at: SystemTime,
}

impl Advertisement {
    fn to_value(&self) -> DiagResult<Value> {
        let mut map = Map::new();
        map.insert(
            "fingerprint".to_string(),
            Value::Array(self.fingerprint.iter().copied().map(Value::from).collect()),
        );
        map.insert(
            "issued_at".to_string(),
            Value::Object(system_time_to_map(self.issued_at)?),
        );
        Ok(Value::Object(map))
    }

    fn from_value(value: Value) -> DiagResult<Self> {
        let mut object = match value {
            Value::Object(map) => map,
            other => {
                return Err(anyhow!(
                    "advertisement must be a JSON object, found {}",
                    describe_json(&other)
                ))
            }
        };
        let fingerprint_value = object
            .remove("fingerprint")
            .ok_or_else(|| anyhow!("advertisement missing fingerprint"))?;
        let issued_at_value = object
            .remove("issued_at")
            .ok_or_else(|| anyhow!("advertisement missing issued_at"))?;
        Ok(Advertisement {
            fingerprint: parse_fingerprint(fingerprint_value)?,
            issued_at: parse_system_time(issued_at_value)?,
        })
    }
}

fn describe_json(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parse_fingerprint(value: Value) -> DiagResult<[u8; 32]> {
    let values = match value {
        Value::Array(values) => values,
        other => {
            return Err(anyhow!(
                "advertisement fingerprint must be an array, found {}",
                describe_json(&other)
            ))
        }
    };
    if values.len() != 32 {
        return Err(anyhow!(
            "advertisement fingerprint must contain 32 entries, found {}",
            values.len()
        ));
    }
    let mut out = [0u8; 32];
    for (idx, value) in values.into_iter().enumerate() {
        out[idx] = parse_byte(value)?;
    }
    Ok(out)
}

fn parse_byte(value: Value) -> DiagResult<u8> {
    let number = match value {
        Value::Number(n) => n,
        Value::String(s) => {
            let parsed: u64 = s
                .parse()
                .map_err(|err| anyhow!("invalid fingerprint byte: {err}"))?;
            return byte_from_u64(parsed);
        }
        other => {
            return Err(anyhow!(
                "fingerprint entries must be numbers, found {}",
                describe_json(&other)
            ))
        }
    };
    let value = number
        .as_u64()
        .ok_or_else(|| anyhow!("fingerprint entries must be unsigned integers"))?;
    byte_from_u64(value)
}

fn byte_from_u64(value: u64) -> DiagResult<u8> {
    if value > u8::MAX as u64 {
        return Err(anyhow!("fingerprint entries must fit in a byte"));
    }
    Ok(value as u8)
}

fn system_time_to_map(time: SystemTime) -> DiagResult<Map> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("timestamp predates unix epoch"))?;
    let mut map = Map::new();
    map.insert(
        "secs_since_epoch".to_string(),
        Value::from(duration.as_secs()),
    );
    map.insert(
        "nanos_since_epoch".to_string(),
        Value::from(duration.subsec_nanos()),
    );
    Ok(map)
}

fn parse_system_time(value: Value) -> DiagResult<SystemTime> {
    let mut object = match value {
        Value::Object(map) => map,
        other => {
            return Err(anyhow!(
                "issued_at must be an object, found {}",
                describe_json(&other)
            ))
        }
    };
    let secs = parse_u64_field(
        object
            .remove("secs_since_epoch")
            .ok_or_else(|| anyhow!("issued_at missing secs_since_epoch"))?,
        "secs_since_epoch",
    )?;
    let nanos = parse_u64_field(
        object
            .remove("nanos_since_epoch")
            .ok_or_else(|| anyhow!("issued_at missing nanos_since_epoch"))?,
        "nanos_since_epoch",
    )?;
    if nanos >= 1_000_000_000 {
        return Err(anyhow!(
            "nanos_since_epoch must be less than 1_000_000_000, found {nanos}"
        ));
    }
    let duration = std::time::Duration::new(secs, nanos as u32);
    Ok(UNIX_EPOCH + duration)
}

fn parse_u64_field(value: Value, field: &str) -> DiagResult<u64> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| anyhow!("{field} must be an unsigned integer")),
        Value::String(s) => s
            .parse::<u64>()
            .map_err(|err| anyhow!("invalid {field}: {err}")),
        other => Err(anyhow!(
            "{field} must be a number, found {}",
            describe_json(&other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_decodes_legacy_json() {
        let json = r#"{"fingerprint":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31],"issued_at":{"secs_since_epoch":123,"nanos_since_epoch":456}}"#;
        let value = json::value_from_str(json).expect("parse legacy json");
        let advert = Advertisement::from_value(value).expect("decode advertisement");
        assert_eq!(advert.fingerprint[0], 0);
        assert_eq!(advert.fingerprint[31], 31);
        let duration = advert
            .issued_at
            .duration_since(UNIX_EPOCH)
            .expect("issued_at after epoch");
        assert_eq!(duration.as_secs(), 123);
        assert_eq!(duration.subsec_nanos(), 456);
    }

    #[test]
    fn advertisement_serializes_expected_shape() {
        let mut fingerprint = [0u8; 32];
        fingerprint[0] = 42;
        fingerprint[31] = 7;
        let advert = Advertisement {
            fingerprint,
            issued_at: UNIX_EPOCH + std::time::Duration::new(5, 9),
        };
        let value = advert.to_value().expect("serialize advertisement");
        let object = match value {
            Value::Object(map) => map,
            other => panic!("expected object, found {:?}", other),
        };
        let fingerprint_value = object.get("fingerprint").expect("fingerprint present");
        let array = match fingerprint_value {
            Value::Array(values) => values,
            _ => panic!("fingerprint not serialized as array"),
        };
        assert_eq!(array.len(), 32);
        assert_eq!(array[0], Value::from(42u8));
        assert_eq!(array[31], Value::from(7u8));
        let issued_at = object.get("issued_at").expect("issued_at present");
        let issued_map = match issued_at {
            Value::Object(map) => map,
            _ => panic!("issued_at not serialized as object"),
        };
        assert_eq!(issued_map.get("secs_since_epoch"), Some(&Value::from(5u64)));
        assert_eq!(
            issued_map.get("nanos_since_epoch"),
            Some(&Value::from(9u32))
        );
    }
}

impl InhouseCertificateStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            current: Arc::new(Mutex::new(None)),
        }
    }

    fn persist(&self, advert: &Advertisement) -> DiagResult<()> {
        let mut file =
            File::create(&self.path).map_err(|err| anyhow!("create cert store: {err}"))?;
        let json_value = advert.to_value()?;
        let json = json::to_vec_value(&json_value);
        file.write_all(&json)
            .map_err(|err| anyhow!("write cert store: {err}"))?;
        file.sync_all()
            .map_err(|err| anyhow!("sync cert store: {err}"))?;
        Ok(())
    }

    fn load(&self) -> DiagResult<Option<Advertisement>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let mut file = File::open(&self.path).map_err(|err| anyhow!("open cert store: {err}"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|err| anyhow!("read cert store: {err}"))?;
        if buf.is_empty() {
            return Ok(None);
        }
        let value =
            json::value_from_slice(&buf).map_err(|err| anyhow!("decode cert store: {err}"))?;
        let advert = Advertisement::from_value(value)?;
        Ok(Some(advert))
    }

    fn generate(&self) -> DiagResult<Advertisement> {
        let certificate = Certificate::generate()?;
        Ok(Advertisement {
            fingerprint: certificate.fingerprint,
            issued_at: SystemTime::now(),
        })
    }
}

impl CertificateStore for InhouseCertificateStore {
    type Advertisement = Advertisement;
    type Error = TbError;

    fn initialize(&self) -> StdResult<Self::Advertisement, Self::Error> {
        let advert = self.generate()?;
        self.persist(&advert)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    fn rotate(&self) -> StdResult<Self::Advertisement, Self::Error> {
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

pub fn verify_remote_certificate(_peer_key: &[u8; 32], cert: &[u8]) -> DiagResult<[u8; 32]> {
    if cert.is_empty() {
        return Err(anyhow!("certificate payload empty"));
    }
    Ok(fingerprint(cert))
}
