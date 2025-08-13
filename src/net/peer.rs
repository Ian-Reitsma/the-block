use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

/// Thread-safe peer set used by the gossip layer.
#[derive(Clone, Default)]
pub struct PeerSet {
    inner: Arc<Mutex<HashSet<SocketAddr>>>,
}

impl PeerSet {
    /// Create a new set seeded with `initial` peers.
    pub fn new(initial: Vec<SocketAddr>) -> Self {
        let set: HashSet<_> = initial.into_iter().collect();
        Self {
            inner: Arc::new(Mutex::new(set)),
        }
    }

    /// Add a peer to the set.
    pub fn add(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(addr);
        }
    }

    /// Return a snapshot of known peers.
    pub fn list(&self) -> Vec<SocketAddr> {
        self.inner
            .lock()
            .map(|g| g.iter().copied().collect())
            .unwrap_or_default()
    }
}
