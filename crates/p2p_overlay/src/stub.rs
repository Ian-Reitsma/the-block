use crate::libp2p_overlay::UptimeTracker;
use crate::{
    Discovery, NoopMetrics, OverlayDiagnostics, OverlayResult, OverlayService, OverlayStore,
    PeerId, UptimeHandle, UptimeInfo, UptimeMetrics, UptimeStore,
};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StubPeerId(Vec<u8>);

impl StubPeerId {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl PeerId for StubPeerId {
    fn from_bytes(bytes: &[u8]) -> OverlayResult<Self> {
        Ok(Self(bytes.to_vec()))
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }
}

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

    pub fn shared(inner: Arc<Mutex<Vec<(P, A)>>>) -> Self {
        Self { inner }
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
    P: PeerId,
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

pub struct StubDiscovery<P, A>
where
    P: PeerId,
    A: Clone + Send + Sync + 'static,
{
    peers: HashMap<P, A>,
    store: MemoryOverlayStore<P, A>,
}

impl<P, A> StubDiscovery<P, A>
where
    P: PeerId,
    A: Clone + Send + Sync + 'static,
{
    pub fn new(store: MemoryOverlayStore<P, A>) -> Self {
        let peers = store.load().unwrap_or_default().into_iter().collect();
        Self { peers, store }
    }
}

impl<P, A> Discovery for StubDiscovery<P, A>
where
    P: PeerId,
    A: Clone + Send + Sync + 'static,
{
    type Peer = P;
    type Address = A;

    fn add_peer(&mut self, peer: Self::Peer, address: Self::Address) {
        self.peers.insert(peer, address);
    }

    fn has_peer(&self, peer: &Self::Peer) -> bool {
        self.peers.contains_key(peer)
    }

    fn persist(&self) {
        let entries: Vec<_> = self
            .peers
            .iter()
            .map(|(p, a)| (p.clone(), a.clone()))
            .collect();
        let _ = self.store.persist(&entries);
    }
}

pub struct StubUptimeStore<P> {
    inner: Mutex<HashMap<P, UptimeInfo>>,
}

impl<P> StubUptimeStore<P>
where
    P: PeerId,
{
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl<P> Default for StubUptimeStore<P>
where
    P: PeerId,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P> UptimeStore<P> for StubUptimeStore<P>
where
    P: PeerId,
{
    fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<P, UptimeInfo>) -> R) -> R {
        let mut guard = self.inner.lock().unwrap();
        f(&mut guard)
    }
}

pub struct StubOverlay<M = NoopMetrics>
where
    M: UptimeMetrics,
{
    store: MemoryOverlayStore<StubPeerId, Vec<u8>>,
    uptime: Arc<UptimeTracker<StubPeerId, StubUptimeStore<StubPeerId>, M>>,
    _metrics: PhantomData<M>,
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
        let uptime_store = StubUptimeStore::new();
        let tracker = Arc::new(UptimeTracker::with_metrics(uptime_store, metrics));
        Self {
            store,
            uptime: tracker,
            _metrics: PhantomData,
        }
    }

    pub fn store(&self) -> MemoryOverlayStore<StubPeerId, Vec<u8>> {
        self.store.clone()
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
    type Peer = StubPeerId;
    type Address = Vec<u8>;

    fn peer_from_bytes(&self, bytes: &[u8]) -> OverlayResult<Self::Peer> {
        Ok(StubPeerId::new(bytes))
    }

    fn peer_to_bytes(&self, peer: &Self::Peer) -> Vec<u8> {
        peer.to_bytes()
    }

    fn discovery(
        &self,
        _local: Self::Peer,
    ) -> Box<dyn Discovery<Peer = Self::Peer, Address = Self::Address> + Send> {
        Box::new(StubDiscovery::new(self.store.clone()))
    }

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>> {
        self.uptime.clone()
    }

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics> {
        Ok(self.snapshot())
    }
}
