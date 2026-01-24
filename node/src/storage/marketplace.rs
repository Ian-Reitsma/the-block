//! Helpers that shape storage-marketplace queries and escrow context.
//!
//! This module lives next to the pipeline/placement helpers so gateway edges can build
//! `storage_market::DiscoveryRequest` arguments without drifting from the on-disk `SimpleDb`
//! snapshots that already track provider quotas, maintenance modes, and telemetry.

use storage_market::DiscoveryRequest;

const DEFAULT_LIMIT: usize = 25;
const MAX_LIMIT: usize = 200;

/// Parameters emitted by the gateway when it wants to discover DHT providers.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub object_size: u64,
    pub shares: u16,
    pub region: Option<String>,
    pub limit: usize,
    pub max_price_per_block: Option<u64>,
    pub min_success_rate_ppm: Option<u64>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            object_size: 0,
            shares: 1,
            region: None,
            limit: DEFAULT_LIMIT,
            max_price_per_block: None,
            min_success_rate_ppm: Some(850_000),
        }
    }
}

impl SearchOptions {
    /// Translate the gateway-friendly search options into the marketplace request.
    pub fn discovery_request(&self) -> DiscoveryRequest {
        DiscoveryRequest {
            object_size: self.object_size,
            shares: self.shares,
            region: self.region.clone(),
            max_price_per_block: self.max_price_per_block,
            min_success_rate_ppm: self.min_success_rate_ppm,
            limit: Self::clamp_limit(self.limit),
        }
    }

    /// Clamp the limit so the DHT query never floods the overlay.
    pub fn clamp_limit(limit: usize) -> usize {
        limit.clamp(1, MAX_LIMIT)
    }
}
