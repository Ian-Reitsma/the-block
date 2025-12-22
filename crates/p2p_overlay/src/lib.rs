#![allow(clippy::new_without_default)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::error::Error;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;

pub type OverlayError = Box<dyn Error + Send + Sync + 'static>;
pub type OverlayResult<T> = Result<T, OverlayError>;

pub trait PeerId: Clone + Eq + Hash + Send + Sync + 'static {
    fn from_bytes(bytes: &[u8]) -> OverlayResult<Self>
    where
        Self: Sized;

    fn to_bytes(&self) -> Vec<u8>;
}

pub trait OverlayStore<P, A>
where
    P: PeerId,
    A: Clone,
{
    fn load(&self) -> OverlayResult<Vec<(P, A)>>;

    fn persist(&self, peers: &[(P, A)]) -> OverlayResult<()>;
}

pub trait Discovery: Send {
    type Peer: PeerId;
    type Address: Clone + Send + Sync + 'static;

    fn add_peer(&mut self, peer: Self::Peer, address: Self::Address);

    fn has_peer(&self, peer: &Self::Peer) -> bool;

    fn persist(&self);
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct UptimeInfo {
    pub last: u64,
    pub total: u64,
    pub claimed_epoch: u64,
}

pub trait UptimeStore<P>: Send + Sync + 'static
where
    P: PeerId,
{
    fn with_map<R>(&self, f: impl FnOnce(&mut HashMap<P, UptimeInfo>) -> R) -> R;
}

pub trait UptimeMetrics: Send + Sync + 'static {
    fn on_claim(&self) {}
    fn on_issue(&self) {}
}

pub struct NoopMetrics;

impl UptimeMetrics for NoopMetrics {}

pub trait UptimeHandle: Send + Sync + 'static {
    type Peer: PeerId;

    fn note_seen(&self, peer: Self::Peer);

    fn eligible(&self, peer: &Self::Peer, threshold: u64, epoch: u64) -> bool;

    fn claim(&self, peer: Self::Peer, threshold: u64, epoch: u64, reward: u64) -> Option<u64>;
}

pub trait OverlayService: Send + Sync + 'static {
    type Peer: PeerId;
    type Address: Clone + Send + Sync + 'static;

    fn peer_from_bytes(&self, bytes: &[u8]) -> OverlayResult<Self::Peer>;

    fn peer_to_bytes(&self, peer: &Self::Peer) -> Vec<u8>;

    fn discovery(
        &self,
        local: Self::Peer,
    ) -> Box<dyn Discovery<Peer = Self::Peer, Address = Self::Address> + Send>;

    fn uptime(&self) -> Arc<dyn UptimeHandle<Peer = Self::Peer>>;

    fn diagnostics(&self) -> OverlayResult<OverlayDiagnostics>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlayDiagnostics {
    pub label: &'static str,
    pub active_peers: usize,
    pub persisted_peers: usize,
    pub database_path: Option<PathBuf>,
}

pub mod inhouse_overlay;
pub mod stub;
pub mod uptime;

pub use inhouse_overlay::{InhouseOverlay, InhouseOverlayStore, InhousePeerId, PeerEndpoint};
pub use stub::{MemoryOverlayStore, StubOverlay};
pub use uptime::{InMemoryUptimeStore, UptimeTracker};
