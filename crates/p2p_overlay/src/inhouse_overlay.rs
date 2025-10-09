use crate::uptime::{InMemoryUptimeStore, UptimeTracker};
use crate::{
    Discovery, NoopMetrics, OverlayDiagnostics, OverlayError, OverlayResult, OverlayService,
    OverlayStore, PeerId, UptimeHandle, UptimeMetrics,
};
use crypto_suite::hashing::blake3::hash;
use foundation_serialization::{
    base58,
    json::{self, Map, Value},
    Deserialize, Serialize,
};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const PEER_ID_LEN: usize = 32;
const CHECKSUM_LEN: usize = 4;

#[derive(Debug)]
enum InhouseOverlayError {
    InvalidPeerLength(usize),
    ChecksumMismatch,
    InvalidEncoding(String),
    InvalidSocket(String),
    Persist(String),
}

impl fmt::Display for InhouseOverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InhouseOverlayError::InvalidPeerLength(len) => {
                write!(f, "invalid peer id length: {len}")
            }
            InhouseOverlayError::ChecksumMismatch => write!(f, "peer checksum mismatch"),
            InhouseOverlayError::InvalidEncoding(err) => {
                write!(f, "invalid peer encoding: {err}")
            }
            InhouseOverlayError::InvalidSocket(addr) => {
                write!(f, "invalid socket address: {addr}")
            }
            InhouseOverlayError::Persist(err) => write!(f, "persist error: {err}"),
        }
    }
}

impl std::error::Error for InhouseOverlayError {}

fn overlay_err(err: InhouseOverlayError) -> OverlayError {
    Box::new(err)
}

fn persist_err(message: impl Into<String>) -> OverlayError {
    overlay_err(InhouseOverlayError::Persist(message.into()))
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct InhousePeerId([u8; PEER_ID_LEN]);

impl InhousePeerId {
    pub fn new(bytes: [u8; PEER_ID_LEN]) -> Self {
        Self(bytes)
    }

    pub fn from_base58(value: &str) -> OverlayResult<Self> {
        let raw = base58::decode(value)
            .map_err(|err| overlay_err(InhouseOverlayError::InvalidEncoding(err.to_string())))?;
        if raw.len() < PEER_ID_LEN + CHECKSUM_LEN {
            return Err(Box::new(InhouseOverlayError::InvalidPeerLength(raw.len())));
        }
        let (payload, checksum) = raw.split_at(PEER_ID_LEN);
        let expected = checksum_bytes(payload);
        let mut provided = [0u8; CHECKSUM_LEN];
        provided.copy_from_slice(checksum);
        if expected != provided {
            return Err(Box::new(InhouseOverlayError::ChecksumMismatch));
        }
        let mut bytes = [0u8; PEER_ID_LEN];
        bytes.copy_from_slice(payload);
        Ok(Self(bytes))
    }

    pub fn to_base58(&self) -> String {
        let mut data = Vec::with_capacity(PEER_ID_LEN + CHECKSUM_LEN);
        data.extend_from_slice(&self.0);
        let checksum = checksum_bytes(&self.0);
        data.extend_from_slice(&checksum);
        base58::encode(&data)
    }

    pub fn as_bytes(&self) -> &[u8; PEER_ID_LEN] {
        &self.0
    }
}

impl PeerId for InhousePeerId {
    fn from_bytes(bytes: &[u8]) -> OverlayResult<Self> {
        if bytes.len() != PEER_ID_LEN {
            return Err(Box::new(InhouseOverlayError::InvalidPeerLength(
                bytes.len(),
            )));
        }
        let mut id = [0u8; PEER_ID_LEN];
        id.copy_from_slice(bytes);
        Ok(Self(id))
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerEndpoint {
    pub socket: SocketAddr,
    pub last_seen: u64,
}

impl PeerEndpoint {
    pub fn new(socket: SocketAddr) -> Self {
        Self {
            socket,
            last_seen: now(),
        }
    }

    pub fn touch(&mut self) {
        self.last_seen = now();
    }
}

#[derive(Clone, Debug)]
pub struct InhouseOverlayStore {
    path: PathBuf,
}

impl InhouseOverlayStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl OverlayStore<InhousePeerId, PeerEndpoint> for InhouseOverlayStore {
    fn load(&self) -> OverlayResult<Vec<(InhousePeerId, PeerEndpoint)>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Vec::new()),
        };
        let value = json::value_from_slice(&bytes).map_err(|err| persist_err(err.to_string()))?;
        extract_peers(value)
    }

    fn persist(&self, peers: &[(InhousePeerId, PeerEndpoint)]) -> OverlayResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| persist_err(err.to_string()))?;
        }
        let mut entries = Vec::with_capacity(peers.len());
        for (peer, endpoint) in peers {
            let mut map = Map::new();
            map.insert("id".to_owned(), Value::from(peer.to_base58()));
            map.insert(
                "address".to_owned(),
                Value::from(endpoint.socket.to_string()),
            );
            map.insert("last_seen".to_owned(), Value::from(endpoint.last_seen));
            entries.push(Value::Object(map));
        }
        let mut root = Map::new();
        root.insert("peers".to_owned(), Value::Array(entries));
        let rendered = json::to_vec_pretty(&Value::Object(root))
            .map_err(|err| persist_err(err.to_string()))?;
        fs::write(&self.path, rendered).map_err(|err| persist_err(err.to_string()))
    }
}

fn extract_peers(value: Value) -> OverlayResult<Vec<(InhousePeerId, PeerEndpoint)>> {
    let entries = match value {
        Value::Object(mut root) => match root.remove("peers") {
            Some(Value::Array(items)) => items,
            Some(Value::Null) | None => Vec::new(),
            Some(other) => {
                return Err(persist_err(format!(
                    "peers must be an array, found {other:?}"
                )))
            }
        },
        Value::Null => Vec::new(),
        other => {
            return Err(persist_err(format!(
                "expected object while loading peers, found {other:?}"
            )))
        }
    };

    let mut peers = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut map = match entry {
            Value::Object(map) => map,
            other => {
                return Err(persist_err(format!(
                    "peer entry must be an object, found {other:?}"
                )))
            }
        };
        let id = take_string(&mut map, "id")?;
        let address = take_string(&mut map, "address")?;
        let last_seen = take_u64(&mut map, "last_seen")?;
        let peer = InhousePeerId::from_base58(&id)?;
        let socket: SocketAddr = address
            .parse()
            .map_err(|_| overlay_err(InhouseOverlayError::InvalidSocket(address.clone())))?;
        peers.push((peer, PeerEndpoint { socket, last_seen }));
    }

    Ok(peers)
}

fn take_string(map: &mut Map, key: &str) -> OverlayResult<String> {
    match map.remove(key) {
        Some(Value::String(value)) => Ok(value),
        Some(other) => Err(persist_err(format!(
            "expected string for field '{key}', found {other:?}"
        ))),
        None => Err(persist_err(format!("missing field '{key}'"))),
    }
}

fn take_u64(map: &mut Map, key: &str) -> OverlayResult<u64> {
    let value = map
        .remove(key)
        .ok_or_else(|| persist_err(format!("missing field '{key}'")))?;
    match value {
        Value::Number(_) | Value::String(_) => json::from_value::<u64>(value)
            .map_err(|err| persist_err(format!("invalid {key}: {err}"))),
        other => Err(persist_err(format!(
            "expected number for field '{key}', found {other:?}"
        ))),
    }
}

pub struct InhouseDiscovery<S>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint>,
{
    local: InhousePeerId,
    peers: HashMap<InhousePeerId, PeerEndpoint>,
    store: S,
}

impl<S> InhouseDiscovery<S>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint>,
{
    pub fn new(local: InhousePeerId, store: S) -> Self {
        let mut peers = HashMap::new();
        if let Ok(entries) = store.load() {
            for (peer, endpoint) in entries {
                peers.insert(peer, endpoint);
            }
        }
        Self {
            local,
            peers,
            store,
        }
    }

    pub fn nearest_peers(
        &self,
        target: &InhousePeerId,
        limit: usize,
    ) -> Vec<(InhousePeerId, PeerEndpoint)> {
        let mut entries: Vec<_> = self
            .peers
            .iter()
            .map(|(peer, endpoint)| (xor_distance(peer, target), peer.clone(), endpoint.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
            .into_iter()
            .take(limit)
            .map(|(_, peer, endpoint)| (peer, endpoint))
            .collect()
    }

    pub fn mark_alive(&mut self, peer: &InhousePeerId) {
        if let Some(entry) = self.peers.get_mut(peer) {
            entry.touch();
        }
    }

    pub fn peers(&self) -> HashMap<InhousePeerId, PeerEndpoint> {
        self.peers.clone()
    }

    fn persist_peers(&self) {
        let entries: Vec<_> = self
            .peers
            .iter()
            .map(|(peer, endpoint)| (peer.clone(), endpoint.clone()))
            .collect();
        let _ = self.store.persist(&entries);
    }
}

impl<S> Discovery for InhouseDiscovery<S>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint> + Send,
{
    type Peer = InhousePeerId;
    type Address = PeerEndpoint;

    fn add_peer(&mut self, peer: Self::Peer, mut address: Self::Address) {
        if peer == self.local {
            return;
        }
        address.touch();
        self.peers.insert(peer, address);
    }

    fn has_peer(&self, peer: &Self::Peer) -> bool {
        self.peers.contains_key(peer)
    }

    fn persist(&self) {
        self.persist_peers();
    }
}

pub struct InhouseOverlay<S = InhouseOverlayStore, M = NoopMetrics>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint> + Clone + Send + Sync + 'static,
    M: UptimeMetrics,
{
    store: S,
    uptime: Arc<UptimeTracker<InhousePeerId, InMemoryUptimeStore<InhousePeerId>, M>>,
    database_path: Option<PathBuf>,
}

impl InhouseOverlay<InhouseOverlayStore, NoopMetrics> {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_metrics(path, NoopMetrics)
    }
}

impl<M> InhouseOverlay<InhouseOverlayStore, M>
where
    M: UptimeMetrics,
{
    pub fn with_metrics(path: impl Into<PathBuf>, metrics: M) -> Self {
        let store = InhouseOverlayStore::new(path.into());
        let database_path = Some(store.path().to_path_buf());
        Self::with_store(store, metrics, database_path)
    }
}

impl<S, M> InhouseOverlay<S, M>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint> + Clone + Send + Sync + 'static,
    M: UptimeMetrics,
{
    pub fn with_store(store: S, metrics: M, database_path: Option<PathBuf>) -> Self {
        let uptime_store = InMemoryUptimeStore::new();
        let tracker = Arc::new(UptimeTracker::with_metrics(uptime_store, metrics));
        Self {
            store,
            uptime: tracker,
            database_path,
        }
    }
}

impl<S, M> OverlayService for InhouseOverlay<S, M>
where
    S: OverlayStore<InhousePeerId, PeerEndpoint> + Clone + Send + Sync + 'static,
    M: UptimeMetrics,
{
    type Peer = InhousePeerId;
    type Address = PeerEndpoint;

    fn peer_from_bytes(&self, bytes: &[u8]) -> OverlayResult<Self::Peer> {
        InhousePeerId::from_bytes(bytes)
    }

    fn peer_to_bytes(&self, peer: &Self::Peer) -> Vec<u8> {
        peer.to_bytes()
    }

    fn discovery(
        &self,
        local: Self::Peer,
    ) -> Box<dyn Discovery<Peer = Self::Peer, Address = Self::Address> + Send> {
        Box::new(InhouseDiscovery::new(local, self.store.clone()))
    }

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>> {
        self.uptime.handle()
    }

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics> {
        let persisted = self.store.load()?.len();
        Ok(OverlayDiagnostics {
            label: "inhouse",
            active_peers: self.uptime.tracked_peers(),
            persisted_peers: persisted,
            database_path: self.database_path.clone(),
        })
    }
}

fn checksum_bytes(data: &[u8]) -> [u8; CHECKSUM_LEN] {
    let digest = hash(data);
    let mut out = [0u8; CHECKSUM_LEN];
    out.copy_from_slice(&digest.as_bytes()[..CHECKSUM_LEN]);
    out
}

fn xor_distance(a: &InhousePeerId, b: &InhousePeerId) -> [u8; PEER_ID_LEN] {
    let mut out = [0u8; PEER_ID_LEN];
    for (i, (lhs, rhs)) in a.as_bytes().iter().zip(b.as_bytes()).enumerate() {
        out[i] = lhs ^ rhs;
    }
    out
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
