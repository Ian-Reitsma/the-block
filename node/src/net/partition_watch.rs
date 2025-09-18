use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use once_cell::sync::Lazy;

#[cfg(feature = "telemetry")]
use crate::telemetry::PARTITION_EVENTS_TOTAL;

/// Tracks peer reachability and detects network partitions.
pub struct PartitionWatch {
    unreachable: Mutex<HashSet<SocketAddr>>,
    threshold: usize,
    active: AtomicBool,
    marker: AtomicU64,
}

impl PartitionWatch {
    /// Create a watcher with the given unreachable peer threshold.
    pub fn new(threshold: usize) -> Self {
        Self {
            unreachable: Mutex::new(HashSet::new()),
            threshold,
            active: AtomicBool::new(false),
            marker: AtomicU64::new(0),
        }
    }

    /// Mark a peer as unreachable.
    pub fn mark_unreachable(&self, peer: SocketAddr) {
        let mut set = self.unreachable.lock().unwrap();
        set.insert(peer);
        if set.len() >= self.threshold && !self.active.swap(true, Ordering::SeqCst) {
            #[cfg(feature = "telemetry")]
            PARTITION_EVENTS_TOTAL.inc();
            self.marker.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Mark a peer as reachable again.
    pub fn mark_reachable(&self, peer: SocketAddr) {
        let mut set = self.unreachable.lock().unwrap();
        set.remove(&peer);
        if set.len() < self.threshold {
            self.active.store(false, Ordering::SeqCst);
        }
    }

    /// Returns true if the node is currently partitioned.
    pub fn is_partitioned(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Current partition marker if partitioned.
    pub fn current_marker(&self) -> Option<u64> {
        if self.is_partitioned() {
            Some(self.marker.load(Ordering::Relaxed))
        } else {
            None
        }
    }
}

/// Global partition watcher used by networking components.
pub static PARTITION_WATCH: Lazy<PartitionWatch> = Lazy::new(|| PartitionWatch::new(8));
