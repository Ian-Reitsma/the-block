//! Network Health Index (NHI)
//!
//! Provides a composite metric of overall network health based on multiple factors:
//! - **Topology**: Connectivity strength, peer distribution, clustering
//! - **Diversity**: Geographic diversity, client version diversity
//! - **Latency**: Average response times, network responsiveness
//! - **Stability**: Peer churn rate, long-term uptime
//!
//! The NHI score ranges from 0.0 (unhealthy) to 1.0 (optimal health).
//! This metric is used by adaptive security systems to adjust parameters.

use concurrency::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Network Health Index configuration
#[derive(Debug, Clone)]
pub struct NetworkHealthConfig {
    /// Weight for topology score (default 0.3)
    pub topology_weight: f64,
    /// Weight for diversity score (default 0.2)
    pub diversity_weight: f64,
    /// Weight for latency score (default 0.25)
    pub latency_weight: f64,
    /// Weight for stability score (default 0.25)
    pub stability_weight: f64,

    /// Target minimum peer count for healthy topology
    pub target_min_peers: usize,
    /// Target optimal peer count
    pub target_optimal_peers: usize,

    /// Churn window for stability calculation (seconds)
    pub churn_window_secs: u64,
    /// Target maximum churn rate (peers/hour)
    pub target_max_churn_rate: f64,

    /// EMA smoothing factor for NHI (0.0 = static, 1.0 = instant)
    pub nhi_smoothing_alpha: f64,
}

impl Default for NetworkHealthConfig {
    fn default() -> Self {
        Self {
            topology_weight: 0.3,
            diversity_weight: 0.2,
            latency_weight: 0.25,
            stability_weight: 0.25,
            target_min_peers: 8,
            target_optimal_peers: 50,
            churn_window_secs: 3600,    // 1 hour
            target_max_churn_rate: 5.0, // 5 peers/hour
            nhi_smoothing_alpha: 0.1,   // Slow smoothing for stability
        }
    }
}

/// Snapshot of network health metrics
#[derive(Debug, Clone)]
pub struct NetworkHealthSnapshot {
    /// Composite health index [0.0, 1.0]
    pub health_index: f64,

    /// Individual component scores
    pub topology_score: f64,
    pub diversity_score: f64,
    pub latency_score: f64,
    pub stability_score: f64,

    /// Supporting metrics
    pub active_peer_count: usize,
    pub avg_latency_ms: f64,
    pub peer_churn_rate: f64, // peers per hour
    pub unique_regions: usize,
    pub unique_client_versions: usize,

    /// Timestamp of snapshot
    pub timestamp: Instant,
}

/// Peer connection event for tracking churn
#[derive(Debug, Clone)]
pub struct PeerEvent {
    pub peer_id: [u8; 32],
    pub event_type: PeerEventType,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerEventType {
    Connected,
    Disconnected,
}

/// Network Health Index tracker
pub struct NetworkHealthTracker {
    config: NetworkHealthConfig,

    /// EMA-smoothed health index
    smoothed_nhi: f64,

    /// Recent peer events for churn calculation
    peer_events: VecDeque<PeerEvent>,

    /// Peer latency tracking (peer_id -> recent latencies)
    latencies: HashMap<[u8; 32], VecDeque<Duration>>,

    /// Peer metadata for diversity tracking
    peer_metadata: HashMap<[u8; 32], PeerMetadata>,

    /// Last snapshot timestamp
    last_snapshot: Instant,
}

#[derive(Debug, Clone)]
struct PeerMetadata {
    region: Option<String>,
    client_version: Option<String>,
}

use std::collections::VecDeque;

impl NetworkHealthTracker {
    pub fn new(config: NetworkHealthConfig) -> Self {
        Self {
            config,
            smoothed_nhi: 0.5, // Start at neutral
            peer_events: VecDeque::new(),
            latencies: HashMap::new(),
            peer_metadata: HashMap::new(),
            last_snapshot: Instant::now(),
        }
    }

    /// Record a peer connection/disconnection event
    pub fn record_peer_event(&mut self, peer_id: [u8; 32], event_type: PeerEventType) {
        let event = PeerEvent {
            peer_id,
            event_type,
            timestamp: Instant::now(),
        };

        // Add to event log
        self.peer_events.push_back(event.clone());

        // Clean old events outside churn window
        let cutoff = Instant::now() - Duration::from_secs(self.config.churn_window_secs);
        while let Some(front) = self.peer_events.front() {
            if front.timestamp < cutoff {
                self.peer_events.pop_front();
            } else {
                break;
            }
        }

        // Update peer metadata
        match event_type {
            PeerEventType::Connected => {
                self.peer_metadata.entry(peer_id).or_insert(PeerMetadata {
                    region: None,
                    client_version: None,
                });
            }
            PeerEventType::Disconnected => {
                self.peer_metadata.remove(&peer_id);
                self.latencies.remove(&peer_id);
            }
        }
    }

    /// Record latency measurement for a peer
    pub fn record_latency(&mut self, peer_id: [u8; 32], latency: Duration) {
        let latencies = self.latencies.entry(peer_id).or_insert_with(VecDeque::new);
        latencies.push_back(latency);

        // Keep only last 20 latency samples per peer
        if latencies.len() > 20 {
            latencies.pop_front();
        }
    }

    /// Update peer metadata (region, client version, etc.)
    pub fn update_peer_metadata(
        &mut self,
        peer_id: [u8; 32],
        region: Option<String>,
        client_version: Option<String>,
    ) {
        if let Some(metadata) = self.peer_metadata.get_mut(&peer_id) {
            if let Some(r) = region {
                metadata.region = Some(r);
            }
            if let Some(v) = client_version {
                metadata.client_version = Some(v);
            }
        }
    }

    /// Compute current network health snapshot
    pub fn compute_health_snapshot(&mut self) -> NetworkHealthSnapshot {
        let topology_score = self.compute_topology_score();
        let diversity_score = self.compute_diversity_score();
        let latency_score = self.compute_latency_score();
        let stability_score = self.compute_stability_score();

        // Weighted composite health index
        let raw_nhi = topology_score * self.config.topology_weight
            + diversity_score * self.config.diversity_weight
            + latency_score * self.config.latency_weight
            + stability_score * self.config.stability_weight;

        // Apply EMA smoothing
        let alpha = self.config.nhi_smoothing_alpha;
        self.smoothed_nhi = alpha * raw_nhi + (1.0 - alpha) * self.smoothed_nhi;

        // Compute supporting metrics
        let active_peer_count = self.peer_metadata.len();
        let avg_latency_ms = self.compute_avg_latency_ms();
        let peer_churn_rate = self.compute_churn_rate();
        let unique_regions = self.count_unique_regions();
        let unique_client_versions = self.count_unique_client_versions();

        let snapshot = NetworkHealthSnapshot {
            health_index: self.smoothed_nhi,
            topology_score,
            diversity_score,
            latency_score,
            stability_score,
            active_peer_count,
            avg_latency_ms,
            peer_churn_rate,
            unique_regions,
            unique_client_versions,
            timestamp: Instant::now(),
        };

        self.last_snapshot = snapshot.timestamp;
        snapshot
    }

    // Individual scoring functions

    fn compute_topology_score(&self) -> f64 {
        let peer_count = self.peer_metadata.len();

        if peer_count == 0 {
            return 0.0;
        }

        // Peer count score: sigmoid-like curve
        // - Below min_peers: poor score
        // - Between min and optimal: linear growth
        // - Above optimal: capped at 1.0
        let peer_score = if peer_count < self.config.target_min_peers {
            (peer_count as f64) / (self.config.target_min_peers as f64) * 0.5
        } else if peer_count < self.config.target_optimal_peers {
            let range = self.config.target_optimal_peers - self.config.target_min_peers;
            let progress = peer_count - self.config.target_min_peers;
            0.5 + 0.5 * (progress as f64) / (range as f64)
        } else {
            1.0
        };

        peer_score.clamp(0.0, 1.0)
    }

    fn compute_diversity_score(&self) -> f64 {
        if self.peer_metadata.is_empty() {
            return 0.0;
        }

        // Geographic diversity component (40%)
        let unique_regions = self.count_unique_regions() as f64;
        let region_diversity = (unique_regions / 10.0).min(1.0); // Target 10+ regions

        // Client version diversity component (60%)
        let unique_versions = self.count_unique_client_versions() as f64;
        let version_diversity = (unique_versions / 5.0).min(1.0); // Target 5+ versions

        (0.4 * region_diversity + 0.6 * version_diversity).clamp(0.0, 1.0)
    }

    fn compute_latency_score(&self) -> f64 {
        let avg_latency_ms = self.compute_avg_latency_ms();

        if avg_latency_ms == 0.0 {
            return 0.5; // No data, neutral score
        }

        // Latency scoring:
        // - <50ms: excellent (1.0)
        // - 50-200ms: good (linear decay)
        // - 200-500ms: acceptable (0.3-0.6)
        // - >500ms: poor (<0.3)
        let score = if avg_latency_ms < 50.0 {
            1.0
        } else if avg_latency_ms < 200.0 {
            1.0 - 0.4 * (avg_latency_ms - 50.0) / 150.0
        } else if avg_latency_ms < 500.0 {
            0.6 - 0.3 * (avg_latency_ms - 200.0) / 300.0
        } else {
            (1.0 / (avg_latency_ms / 100.0)).max(0.1)
        };

        score.clamp(0.0, 1.0)
    }

    fn compute_stability_score(&self) -> f64 {
        let churn_rate = self.compute_churn_rate();

        // Stability scoring based on churn rate
        // - 0 churn: perfect (1.0)
        // - Below target: excellent (0.8-1.0)
        // - At target: acceptable (0.5)
        // - Above target: poor (<0.5)
        let score = if churn_rate < self.config.target_max_churn_rate {
            0.8 + 0.2 * (1.0 - churn_rate / self.config.target_max_churn_rate)
        } else {
            0.5 * (self.config.target_max_churn_rate / churn_rate).min(1.0)
        };

        score.clamp(0.0, 1.0)
    }

    // Helper functions

    fn compute_avg_latency_ms(&self) -> f64 {
        if self.latencies.is_empty() {
            return 0.0;
        }

        let mut total_samples = 0u64;
        let mut total_ms = 0.0;

        for latencies in self.latencies.values() {
            for latency in latencies {
                total_ms += latency.as_secs_f64() * 1000.0;
                total_samples += 1;
            }
        }

        if total_samples == 0 {
            0.0
        } else {
            total_ms / (total_samples as f64)
        }
    }

    fn compute_churn_rate(&self) -> f64 {
        // Count disconnect events in the churn window
        let disconnect_count = self
            .peer_events
            .iter()
            .filter(|e| e.event_type == PeerEventType::Disconnected)
            .count();

        // Convert to peers per hour
        let window_hours = (self.config.churn_window_secs as f64) / 3600.0;
        if window_hours > 0.0 {
            (disconnect_count as f64) / window_hours
        } else {
            0.0
        }
    }

    fn count_unique_regions(&self) -> usize {
        self.peer_metadata
            .values()
            .filter_map(|m| m.region.as_ref())
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    fn count_unique_client_versions(&self) -> usize {
        self.peer_metadata
            .values()
            .filter_map(|m| m.client_version.as_ref())
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// Get the current smoothed health index without recomputing
    pub fn current_health_index(&self) -> f64 {
        self.smoothed_nhi
    }
}

/// Global network health tracker instance
static NETWORK_HEALTH: Lazy<Arc<Mutex<NetworkHealthTracker>>> = Lazy::new(|| {
    Arc::new(Mutex::new(NetworkHealthTracker::new(
        NetworkHealthConfig::default(),
    )))
});

/// Get the global network health tracker
pub fn global_health_tracker() -> Arc<Mutex<NetworkHealthTracker>> {
    Arc::clone(&NETWORK_HEALTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_score_empty() {
        let tracker = NetworkHealthTracker::new(NetworkHealthConfig::default());
        assert_eq!(tracker.compute_topology_score(), 0.0);
    }

    #[test]
    fn test_topology_score_progression() {
        let config = NetworkHealthConfig {
            target_min_peers: 10,
            target_optimal_peers: 50,
            ..Default::default()
        };
        let mut tracker = NetworkHealthTracker::new(config);

        // Add 5 peers (half of min) → score should be ~0.25
        for i in 0..5 {
            let peer_id = [i as u8; 32];
            tracker.record_peer_event(peer_id, PeerEventType::Connected);
        }
        let score_5 = tracker.compute_topology_score();
        assert!(score_5 > 0.2 && score_5 < 0.3, "Got {}", score_5);

        // Add 5 more (at min) → score should be 0.5
        for i in 5..10 {
            let peer_id = [i as u8; 32];
            tracker.record_peer_event(peer_id, PeerEventType::Connected);
        }
        let score_10 = tracker.compute_topology_score();
        assert_eq!(score_10, 0.5);

        // Add to optimal (50) → score should be 1.0
        for i in 10..50 {
            let peer_id = [i as u8; 32];
            tracker.record_peer_event(peer_id, PeerEventType::Connected);
        }
        let score_50 = tracker.compute_topology_score();
        assert_eq!(score_50, 1.0);
    }

    #[test]
    fn test_churn_rate_calculation() {
        let mut tracker = NetworkHealthTracker::new(NetworkHealthConfig::default());

        // Connect 10 peers
        for i in 0..10 {
            tracker.record_peer_event([i; 32], PeerEventType::Connected);
        }

        // Disconnect 5 peers
        for i in 0..5 {
            tracker.record_peer_event([i; 32], PeerEventType::Disconnected);
        }

        let churn_rate = tracker.compute_churn_rate();
        // 5 disconnects in 1 hour window → 5 peers/hour
        assert_eq!(churn_rate, 5.0);
    }

    #[test]
    fn test_latency_scoring() {
        let mut tracker = NetworkHealthTracker::new(NetworkHealthConfig::default());

        // Add peer with excellent latency (30ms)
        let peer_id = [1; 32];
        tracker.record_peer_event(peer_id, PeerEventType::Connected);
        tracker.record_latency(peer_id, Duration::from_millis(30));

        let score = tracker.compute_latency_score();
        assert_eq!(score, 1.0, "Latency <50ms should score 1.0");
    }

    #[test]
    fn test_diversity_scoring() {
        let mut tracker = NetworkHealthTracker::new(NetworkHealthConfig::default());

        // Add peers with diverse regions and versions
        for i in 0..10 {
            let peer_id = [i; 32];
            tracker.record_peer_event(peer_id, PeerEventType::Connected);
            tracker.update_peer_metadata(
                peer_id,
                Some(format!("region-{}", i % 5)), // 5 unique regions
                Some(format!("v1.{}", i % 3)),     // 3 unique versions
            );
        }

        let score = tracker.compute_diversity_score();
        // 5 regions/10 target = 0.5, 3 versions/5 target = 0.6
        // Combined: 0.4 × 0.5 + 0.6 × 0.6 = 0.56
        assert!((score - 0.56).abs() < 0.01, "Got diversity score {}", score);
    }

    #[test]
    fn test_health_index_smoothing() {
        let config = NetworkHealthConfig {
            nhi_smoothing_alpha: 0.5, // Fast smoothing for testing
            ..Default::default()
        };
        let mut tracker = NetworkHealthTracker::new(config);

        // Initial health index
        let snapshot1 = tracker.compute_health_snapshot();
        let initial_nhi = snapshot1.health_index;

        // Add peers to improve health
        for i in 0..20 {
            tracker.record_peer_event([i; 32], PeerEventType::Connected);
        }

        let snapshot2 = tracker.compute_health_snapshot();
        let improved_nhi = snapshot2.health_index;

        // NHI should improve but be smoothed (not instant jump)
        assert!(improved_nhi > initial_nhi);
        assert!(improved_nhi < 1.0); // Not instant perfection due to EMA
    }
}
