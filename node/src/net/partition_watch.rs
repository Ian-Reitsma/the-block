use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use concurrency::Lazy;

use super::OverlayPeerId;
#[cfg(feature = "telemetry")]
use crate::telemetry::PARTITION_EVENTS_TOTAL;
use p2p_overlay::PeerId;

/// Tracks peer reachability and detects network partitions.
pub struct PartitionWatch {
    unreachable: Mutex<HashSet<OverlayPeerId>>,
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
    pub fn mark_unreachable(&self, peer: OverlayPeerId) {
        let mut set = self.unreachable.lock().unwrap();
        set.insert(peer);
        if set.len() >= self.threshold && !self.active.swap(true, Ordering::SeqCst) {
            #[cfg(feature = "telemetry")]
            PARTITION_EVENTS_TOTAL.inc();
            self.marker.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Mark a peer as reachable again.
    pub fn mark_reachable(&self, peer: OverlayPeerId) {
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

    /// Snapshot the set of peers currently considered unreachable.
    pub fn isolated_peers(&self) -> Vec<OverlayPeerId> {
        let mut peers = self
            .unreachable
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        peers.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
        peers
    }

    /// Returns true if the provided peer is currently marked unreachable.
    pub fn is_isolated(&self, peer: &OverlayPeerId) -> bool {
        self.unreachable.lock().unwrap().contains(peer)
    }
}

/// Global partition watcher used by networking components.
pub static PARTITION_WATCH: Lazy<PartitionWatch> = Lazy::new(|| PartitionWatch::new(8));

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(byte: u8) -> OverlayPeerId {
        crate::net::overlay_peer_from_bytes(&[byte; 32]).expect("overlay peer")
    }

    #[test]
    fn threshold_crossing_tracks_marker() {
        let watch = PartitionWatch::new(2);
        let a = peer(1);
        let b = peer(2);

        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);

        watch.mark_unreachable(a.clone());
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);

        watch.mark_unreachable(b.clone());
        assert!(watch.is_partitioned());
        let marker = watch.current_marker().expect("marker set");
        assert!(marker > 0);
        assert!(watch.is_isolated(&a));
        assert!(watch.is_isolated(&b));

        watch.mark_reachable(a.clone());
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);
        assert!(!watch.is_isolated(&a));
        assert!(watch.is_isolated(&b));

        watch.mark_reachable(b);
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);
    }

    #[test]
    fn isolated_peers_are_deduplicated_and_sorted() {
        let watch = PartitionWatch::new(3);
        let peers = vec![peer(9), peer(3), peer(200)];
        for p in &peers {
            watch.mark_unreachable(p.clone());
            watch.mark_unreachable(p.clone());
        }
        let mut expected = peers
            .iter()
            .map(|p| p.as_bytes().to_vec())
            .collect::<Vec<_>>();
        expected.sort();
        let actual = watch.isolated_peers();
        assert_eq!(actual.len(), expected.len());
        for (idx, peer_id) in actual.iter().enumerate() {
            assert_eq!(peer_id.as_bytes(), expected[idx].as_slice());
        }
    }

    #[test]
    fn overlay_id_roundtrip_preserves_partition_flags() {
        let watch = PartitionWatch::new(1);
        let raw = [42u8; 32];
        let overlay = crate::net::overlay_peer_from_bytes(&raw).expect("overlay peer");
        let encoded = crate::net::overlay_peer_to_base58(&overlay);
        let decoded = crate::net::overlay_peer_from_base58(&encoded).expect("decoded overlay");

        watch.mark_unreachable(decoded.clone());
        assert!(watch.is_partitioned());
        assert!(watch.is_isolated(&overlay));
        assert!(watch.is_isolated(&decoded));
        let isolated = watch.isolated_peers();
        assert_eq!(isolated.len(), 1);
        assert_eq!(isolated[0].as_bytes(), overlay.as_bytes());

        watch.mark_reachable(overlay);
        assert!(!watch.is_partitioned());
        assert!(watch.isolated_peers().is_empty());
    }

    #[cfg(not(feature = "telemetry"))]
    #[test]
    fn threshold_reset_without_telemetry_never_sets_marker() {
        let watch = PartitionWatch::new(3);
        let a = peer(11);
        let b = peer(22);

        watch.mark_unreachable(a.clone());
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);

        watch.mark_unreachable(b.clone());
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);

        watch.mark_reachable(b);
        assert!(!watch.is_partitioned());
        assert_eq!(watch.current_marker(), None);

        watch.mark_reachable(a);
        assert!(!watch.is_partitioned());
        assert!(watch.isolated_peers().is_empty());
    }
}
