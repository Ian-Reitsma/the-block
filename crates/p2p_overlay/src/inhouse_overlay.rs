use crate::uptime::{InMemoryUptimeStore, UptimeTracker};
use crate::{
    Discovery, NoopMetrics, OverlayDiagnostics, OverlayError, OverlayResult, OverlayService,
    OverlayStore, PeerId, UptimeHandle, UptimeMetrics,
};
use blake3::hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const PEER_ID_LEN: usize = 32;
const CHECKSUM_LEN: usize = 4;

#[derive(Debug, Error)]
enum InhouseOverlayError {
    #[error("invalid peer id length: {0}")]
    InvalidPeerLength(usize),
    #[error("peer checksum mismatch")]
    ChecksumMismatch,
    #[error("invalid peer encoding: {0}")]
    InvalidEncoding(String),
    #[error("invalid socket address: {0}")]
    InvalidSocket(String),
    #[error("persist error: {0}")]
    Persist(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct InhousePeerId(
    #[serde(with = "crate::inhouse_overlay::serde_bytes32")] [u8; PEER_ID_LEN],
);

impl InhousePeerId {
    pub fn new(bytes: [u8; PEER_ID_LEN]) -> Self {
        Self(bytes)
    }

    pub fn from_base58(value: &str) -> OverlayResult<Self> {
        let raw = bs58::decode(value).into_vec().map_err(|e| {
            OverlayError::from(Box::new(InhouseOverlayError::InvalidEncoding(
                e.to_string(),
            )))
        })?;
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
        bs58::encode(data).into_string()
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

#[derive(Default, Serialize, Deserialize)]
struct PersistedPeers {
    peers: Vec<PersistedPeer>,
}

#[derive(Serialize, Deserialize)]
struct PersistedPeer {
    id: String,
    address: String,
    last_seen: u64,
}

impl OverlayStore<InhousePeerId, PeerEndpoint> for InhouseOverlayStore {
    fn load(&self) -> OverlayResult<Vec<(InhousePeerId, PeerEndpoint)>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Vec::new()),
        };
        let persisted: PersistedPeers = serde_json::from_slice(&bytes).map_err(|e| {
            OverlayError::from(Box::new(InhouseOverlayError::Persist(e.to_string())))
        })?;
        let mut peers = Vec::with_capacity(persisted.peers.len());
        for entry in persisted.peers {
            let peer = InhousePeerId::from_base58(&entry.id)?;
            let socket: SocketAddr = entry.address.parse().map_err(|_| {
                Box::new(InhouseOverlayError::InvalidSocket(entry.address.clone())) as OverlayError
            })?;
            peers.push((
                peer,
                PeerEndpoint {
                    socket,
                    last_seen: entry.last_seen,
                },
            ));
        }
        Ok(peers)
    }

    fn persist(&self, peers: &[(InhousePeerId, PeerEndpoint)]) -> OverlayResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Box::new(InhouseOverlayError::Persist(e.to_string())) as OverlayError
            })?;
        }
        let persisted = PersistedPeers {
            peers: peers
                .iter()
                .map(|(peer, endpoint)| PersistedPeer {
                    id: peer.to_base58(),
                    address: endpoint.socket.to_string(),
                    last_seen: endpoint.last_seen,
                })
                .collect(),
        };
        let data = serde_json::to_vec_pretty(&persisted)
            .map_err(|e| Box::new(InhouseOverlayError::Persist(e.to_string())) as OverlayError)?;
        fs::write(&self.path, data)
            .map_err(|e| Box::new(InhouseOverlayError::Persist(e.to_string())) as OverlayError)
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

mod serde_bytes32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = <Vec<u8>>::deserialize(deserializer)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::invalid_length(bytes.len(), &"32"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(out)
    }
}
