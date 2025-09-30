use crate::inhouse_overlay::{InhousePeerId, PeerEndpoint};
use crate::uptime::{InMemoryUptimeStore, UptimeTracker};
use crate::{
    Discovery, NoopMetrics, OverlayDiagnostics, OverlayResult, OverlayService, OverlayStore,
    PeerId, UptimeHandle, UptimeMetrics,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MemoryOverlayStore<P, A> {
    inner: Arc<Mutex<Vec<(P, A)>>>,
}

impl<P, A> MemoryOverlayStore<P, A> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn snapshot(&self) -> Vec<(P, A)>
    where
        P: Clone,
        A: Clone,
    {
        self.inner.lock().unwrap().clone()
    }
}

impl<P, A> Default for MemoryOverlayStore<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P, A> OverlayStore<P, A> for MemoryOverlayStore<P, A>
where
    P: PeerId + Clone,
    A: Clone,
{
    fn load(&self) -> OverlayResult<Vec<(P, A)>> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn persist(&self, peers: &[(P, A)]) -> OverlayResult<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.clear();
        guard.extend(peers.iter().cloned());
        Ok(())
    }
}

pub struct StubDiscovery {
    peers: HashMap<InhousePeerId, PeerEndpoint>,
    store: MemoryOverlayStore<InhousePeerId, PeerEndpoint>,
}

impl StubDiscovery {
    pub fn new(store: MemoryOverlayStore<InhousePeerId, PeerEndpoint>) -> Self {
        let peers = store.load().unwrap_or_default().into_iter().collect();
        Self { peers, store }
    }

    pub fn peers(&self) -> HashMap<InhousePeerId, PeerEndpoint> {
        self.peers.clone()
    }
}

impl Discovery for StubDiscovery {
    type Peer = InhousePeerId;
    type Address = PeerEndpoint;

    fn add_peer(&mut self, peer: Self::Peer, mut address: Self::Address) {
        address.touch();
        self.peers.insert(peer, address);
    }

    fn has_peer(&self, peer: &Self::Peer) -> bool {
        self.peers.contains_key(peer)
    }

    fn persist(&self) {
        let entries: Vec<_> = self
            .peers
            .iter()
            .map(|(peer, endpoint)| (peer.clone(), endpoint.clone()))
            .collect();
        let _ = self.store.persist(&entries);
    }
}

pub struct StubOverlay<M = NoopMetrics>
where
    M: UptimeMetrics,
{
    store: MemoryOverlayStore<InhousePeerId, PeerEndpoint>,
    uptime: Arc<UptimeTracker<InhousePeerId, InMemoryUptimeStore<InhousePeerId>, M>>,
}

impl StubOverlay<NoopMetrics> {
    pub fn new() -> Self {
        Self::with_metrics(NoopMetrics)
    }
}

impl<M> StubOverlay<M>
where
    M: UptimeMetrics,
{
    pub fn with_metrics(metrics: M) -> Self {
        let store = MemoryOverlayStore::new();
        let uptime_store = InMemoryUptimeStore::new();
        let tracker = Arc::new(UptimeTracker::with_metrics(uptime_store, metrics));
        Self {
            store,
            uptime: tracker,
        }
    }

    pub fn store(&self) -> MemoryOverlayStore<InhousePeerId, PeerEndpoint> {
        self.store.clone()
    }

    pub fn discovery(&self, _local: InhousePeerId) -> StubDiscovery {
        StubDiscovery::new(self.store.clone())
    }

    pub fn snapshot(&self) -> OverlayDiagnostics {
        OverlayDiagnostics {
            label: "stub",
            active_peers: self.uptime.tracked_peers(),
            persisted_peers: self.store.snapshot().len(),
            database_path: None,
        }
    }
}

impl<M> OverlayService for StubOverlay<M>
where
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
        Box::new(self.discovery(local))
    }

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>> {
        self.uptime.handle()
    }

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics> {
        Ok(self.snapshot())
    }
}
