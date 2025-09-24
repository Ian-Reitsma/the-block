use crate::{
    Discovery, NoopMetrics, OverlayDiagnostics, OverlayError, OverlayResult, OverlayService,
    OverlayStore, PeerId, UptimeHandle, UptimeInfo, UptimeMetrics, UptimeStore,
};
use libp2p::kad::{store::MemoryStore, Behaviour as KadBehaviour, Config as KadConfig};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    peers: Vec<(Vec<u8>, Vec<u8>)>,
}

pub type Libp2pMultiaddr = Multiaddr;
pub type Libp2pPeerId = libp2p::PeerId;

impl PeerId for Libp2pPeerId {
    fn from_bytes(bytes: &[u8]) -> OverlayResult<Self> {
        libp2p::PeerId::from_bytes(bytes).map_err(|err| -> OverlayError { Box::new(err) })
    }

    fn to_bytes(&self) -> Vec<u8> {
        libp2p::PeerId::to_bytes(self.clone())
    }
}

#[derive(Clone, Debug)]
pub struct FileOverlayStore {
    path: PathBuf,
}

impl FileOverlayStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl OverlayStore<Libp2pPeerId, Libp2pMultiaddr> for FileOverlayStore {
    fn load(&self) -> OverlayResult<Vec<(Libp2pPeerId, Libp2pMultiaddr)>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Vec::new()),
        };
        let persisted: Persisted = match bincode::deserialize(&bytes) {
            Ok(p) => p,
            Err(_) => return Ok(Vec::new()),
        };
        let mut peers = Vec::with_capacity(persisted.peers.len());
        for (pid, addr_bytes) in persisted.peers {
            if let Ok(peer) = Libp2pPeerId::from_bytes(&pid) {
                match Libp2pMultiaddr::try_from(addr_bytes) {
                    Ok(addr) => peers.push((peer, addr)),
                    Err(err) => {
                        let _ = err;
                    }
                }
            }
        }
        Ok(peers)
    }

    fn persist(&self, peers: &[(Libp2pPeerId, Libp2pMultiaddr)]) -> OverlayResult<()> {
        let list: Vec<(Vec<u8>, Vec<u8>)> = peers
            .iter()
            .map(|(p, a)| (p.to_bytes(), a.clone().to_vec()))
            .collect();
        let bytes = bincode::serialize(&Persisted { peers: list })
            .map_err(|err| -> OverlayError { Box::new(err) })?;
        fs::write(&self.path, bytes).map_err(|err| -> OverlayError { Box::new(err) })
    }
}

pub struct Libp2pDiscovery<S>
where
    S: OverlayStore<Libp2pPeerId, Libp2pMultiaddr>,
{
    kademlia: KadBehaviour<MemoryStore>,
    store: S,
    peers: HashMap<Libp2pPeerId, Libp2pMultiaddr>,
}

impl<S> Libp2pDiscovery<S>
where
    S: OverlayStore<Libp2pPeerId, Libp2pMultiaddr>,
{
    pub fn new(local: Libp2pPeerId, store: S) -> Self {
        let cfg = KadConfig::default();
        let memory = MemoryStore::new(local);
        let mut kademlia = KadBehaviour::with_config(local, memory, cfg);
        let mut peers = HashMap::new();
        if let Ok(entries) = store.load() {
            for (peer, addr) in entries {
                kademlia.add_address(&peer, addr.clone());
                peers.insert(peer, addr);
            }
        }
        Self {
            kademlia,
            store,
            peers,
        }
    }

    pub fn kademlia(&mut self) -> &mut KadBehaviour<MemoryStore> {
        &mut self.kademlia
    }
}

impl Libp2pDiscovery<FileOverlayStore> {
    pub fn with_persistent_path(local: Libp2pPeerId, path: impl Into<PathBuf>) -> Self {
        Self::new(local, FileOverlayStore::new(path))
    }
}

impl<S> Discovery for Libp2pDiscovery<S>
where
    S: OverlayStore<Libp2pPeerId, Libp2pMultiaddr> + Send,
{
    type Peer = Libp2pPeerId;
    type Address = Libp2pMultiaddr;

    fn add_peer(&mut self, peer: Self::Peer, address: Self::Address) {
        if self.peers.insert(peer, address.clone()).is_none() {
            self.kademlia.add_address(&peer, address);
        }
    }

    fn has_peer(&self, peer: &Self::Peer) -> bool {
        self.peers.contains_key(peer)
    }

    fn persist(&self) {
        let peers: Vec<_> = self
            .peers
            .iter()
            .map(|(p, a)| (p.clone(), a.clone()))
            .collect();
        let _ = self.store.persist(&peers);
    }
}

pub struct InMemoryUptimeStore<P>
where
    P: PeerId,
{
    inner: Mutex<HashMap<P, UptimeInfo>>,
}

impl<P> InMemoryUptimeStore<P>
where
    P: PeerId,
{
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl<P> Default for InMemoryUptimeStore<P>
where
    P: PeerId,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P> UptimeStore<P> for InMemoryUptimeStore<P>
where
    P: PeerId,
{
    fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<P, UptimeInfo>) -> R) -> R {
        let mut guard = self.inner.lock().expect("uptime map poisoned");
        f(&mut guard)
    }
}

pub struct UptimeTracker<P, S, M = NoopMetrics>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    store: S,
    metrics: M,
    _marker: PhantomData<fn() -> P>,
}

impl<P, S> UptimeTracker<P, S, NoopMetrics>
where
    P: PeerId,
    S: UptimeStore<P>,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            metrics: NoopMetrics,
            _marker: PhantomData,
        }
    }
}

impl<P, S, M> UptimeTracker<P, S, M>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    pub fn with_metrics(store: S, metrics: M) -> Self {
        Self {
            store,
            metrics,
            _marker: PhantomData,
        }
    }

    pub fn note_seen(&self, peer: P) {
        self.store.with_map(|map| {
            let entry = map.entry(peer).or_default();
            let n = now();
            if entry.last > 0 {
                entry.total = entry.total.saturating_add(n - entry.last);
            }
            entry.last = n;
        });
    }

    pub fn eligible(&self, peer: &P, threshold: u64, epoch: u64) -> bool {
        self.store.with_map(|map| {
            map.get(peer)
                .map(|info| info.total >= threshold && info.claimed_epoch < epoch)
                .unwrap_or(false)
        })
    }

    pub fn claim(&self, peer: P, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
        let issued = self.store.with_map(|map| {
            let info = map.entry(peer).or_default();
            if info.total >= threshold && info.claimed_epoch < epoch {
                info.claimed_epoch = epoch;
                true
            } else {
                false
            }
        });
        if issued {
            self.metrics.on_claim();
            self.metrics.on_issue();
            Some(reward)
        } else {
            None
        }
    }

    pub fn tracked_peers(&self) -> usize {
        self.store.with_map(|map| map.len())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl<P, S, M> UptimeHandle for UptimeTracker<P, S, M>
where
    P: PeerId,
    S: UptimeStore<P>,
    M: UptimeMetrics,
{
    type Peer = P;

    fn note_seen(&self, peer: Self::Peer) {
        UptimeTracker::note_seen(self, peer);
    }

    fn eligible(&self, peer: &Self::Peer, threshold: u64, epoch: u64) -> bool {
        UptimeTracker::eligible(self, peer, threshold, epoch)
    }

    fn claim(&self, peer: Self::Peer, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
        UptimeTracker::claim(self, peer, threshold, epoch, reward)
    }
}

pub struct Libp2pOverlay<M = NoopMetrics>
where
    M: UptimeMetrics,
{
    store_path: PathBuf,
    uptime: Arc<UptimeTracker<Libp2pPeerId, InMemoryUptimeStore<Libp2pPeerId>, M>>,
    _metrics: PhantomData<M>,
}

impl Libp2pOverlay<NoopMetrics> {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_metrics(path, NoopMetrics)
    }
}

impl<M> Libp2pOverlay<M>
where
    M: UptimeMetrics,
{
    pub fn with_metrics(path: impl Into<PathBuf>, metrics: M) -> Self {
        let store_path = path.into();
        let store = InMemoryUptimeStore::new();
        let tracker = Arc::new(UptimeTracker::with_metrics(store, metrics));
        Self {
            store_path,
            uptime: tracker,
            _metrics: PhantomData,
        }
    }
}

impl<M> OverlayService for Libp2pOverlay<M>
where
    M: UptimeMetrics,
{
    type Peer = Libp2pPeerId;
    type Address = Libp2pMultiaddr;

    fn peer_from_bytes(&self, bytes: &[u8]) -> OverlayResult<Self::Peer> {
        Libp2pPeerId::from_bytes(bytes).map_err(|err| -> OverlayError { Box::new(err) })
    }

    fn peer_to_bytes(&self, peer: &Self::Peer) -> Vec<u8> {
        peer.to_bytes()
    }

    fn discovery(
        &self,
        local: Self::Peer,
    ) -> Box<dyn Discovery<Peer = Self::Peer, Address = Self::Address> + Send> {
        Box::new(Libp2pDiscovery::with_persistent_path(
            local,
            self.store_path.clone(),
        ))
    }

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>> {
        self.uptime.clone()
    }

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics> {
        let store = FileOverlayStore::new(self.store_path.clone());
        let persisted = store.load()?.len();
        Ok(OverlayDiagnostics {
            label: "libp2p",
            active_peers: self.uptime.tracked_peers(),
            persisted_peers: persisted,
            database_path: Some(self.store_path.clone()),
        })
    }
}
