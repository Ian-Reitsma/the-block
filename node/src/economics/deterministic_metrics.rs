//! Deterministic market metric derivation from on-chain settlement receipts.
//!
//! This module replaces placeholder hashing with actual receipt parsing to compute
//! utilization and provider margins deterministically from block contents.
//!
//! # Determinism Contract
//! Given identical receipt lists and block ranges, all nodes compute identical
//! metrics. This is critical for consensus - the metrics feed into Launch Governor
//! gates that make network decisions.

use super::{MarketMetric, MarketMetrics};
use crate::receipts::Receipt;
use crate::Block;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashSet;

/// Market metric snapshot for telemetry (parts per million format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EconomicsPrevMetric {
    pub market: String,
    pub utilization_ppm: i64,
    pub provider_margin_ppm: i64,
}

/// Convert MarketMetrics to a vector of telemetry-friendly snapshots.
///
/// This converts f64 ratios to parts-per-million integers for Prometheus metrics.
pub fn snapshot_from_metrics(metrics: &MarketMetrics) -> Vec<EconomicsPrevMetric> {
    vec![
        EconomicsPrevMetric {
            market: "storage".to_string(),
            utilization_ppm: (metrics.storage.utilization * 1_000_000.0) as i64,
            provider_margin_ppm: (metrics.storage.provider_margin * 1_000_000.0) as i64,
        },
        EconomicsPrevMetric {
            market: "compute".to_string(),
            utilization_ppm: (metrics.compute.utilization * 1_000_000.0) as i64,
            provider_margin_ppm: (metrics.compute.provider_margin * 1_000_000.0) as i64,
        },
        EconomicsPrevMetric {
            market: "energy".to_string(),
            utilization_ppm: (metrics.energy.utilization * 1_000_000.0) as i64,
            provider_margin_ppm: (metrics.energy.provider_margin * 1_000_000.0) as i64,
        },
        EconomicsPrevMetric {
            market: "ad".to_string(),
            utilization_ppm: (metrics.ad.utilization * 1_000_000.0) as i64,
            provider_margin_ppm: (metrics.ad.provider_margin * 1_000_000.0) as i64,
        },
    ]
}

/// Derive market metrics from settlement receipts in the chain.
///
/// Processes receipts from all blocks in [epoch_start, epoch_end) and computes
/// utilization and provider margin metrics for each market domain.
///
/// # Arguments
/// * `chain` - Complete chain slice from genesis to tip
/// * `epoch_start` - Epoch start block height (inclusive)
/// * `epoch_end` - Epoch end block height (exclusive)
///
/// # Returns
/// MarketMetrics with utilization_ppm and provider_margin_ppm for each market
pub fn derive_market_metrics_from_chain(
    chain: &[Block],
    epoch_start: u64,
    epoch_end: u64,
) -> MarketMetrics {
    let mut storage_state = MarketState::default();
    let mut compute_state = MarketState::default();
    let mut energy_state = MarketState::default();
    let mut ad_state = MarketState::default();

    // Validate epoch bounds
    let epoch_start_usize = epoch_start as usize;
    let epoch_end_usize = epoch_end.min(chain.len() as u64) as usize;

    // Only iterate blocks in the epoch window (performance optimization)
    // This changes from O(chain_length) to O(epoch_window_size)
    if epoch_start_usize >= chain.len() || epoch_start_usize >= epoch_end_usize {
        // Empty epoch window - return default metrics
        return MarketMetrics {
            storage: compute_market_metric(&storage_state, true),
            compute: compute_market_metric(&compute_state, false),
            energy: compute_market_metric(&energy_state, false),
            ad: compute_ad_metric(&ad_state),
        };
    }

    for block in &chain[epoch_start_usize..epoch_end_usize] {
        // Access receipts field in Block
        for receipt in &block.receipts {
            match receipt {
                Receipt::Storage(r) => {
                    storage_state.revenue_ct = storage_state.revenue_ct.saturating_add(r.price);
                    storage_state.capacity = storage_state.capacity.saturating_add(r.bytes);
                    storage_state.provider_escrow = storage_state
                        .provider_escrow
                        .saturating_add(r.provider_escrow);
                    storage_state.settlement_count += 1;
                    storage_state.providers.insert(r.provider.clone());
                }
                Receipt::Compute(r) => {
                    compute_state.revenue_ct = compute_state.revenue_ct.saturating_add(r.payment);
                    compute_state.capacity = compute_state.capacity.saturating_add(r.compute_units);
                    if r.verified {
                        compute_state.verified_count += 1;
                    }
                    compute_state.settlement_count += 1;
                    compute_state.providers.insert(r.provider.clone());
                }
                Receipt::Energy(r) => {
                    energy_state.revenue_ct = energy_state.revenue_ct.saturating_add(r.price);
                    energy_state.capacity = energy_state.capacity.saturating_add(r.energy_units);
                    energy_state.settlement_count += 1;
                    energy_state.providers.insert(r.provider.clone());
                }
                Receipt::Ad(r) => {
                    ad_state.revenue_ct = ad_state.revenue_ct.saturating_add(r.spend);
                    ad_state.impressions = ad_state.impressions.saturating_add(r.impressions);
                    ad_state.conversions =
                        ad_state.conversions.saturating_add(r.conversions as u64);
                    ad_state.settlement_count += 1;
                    ad_state.providers.insert(r.publisher.clone());
                }
            }
        }
    }

    // Compute metrics for each market
    MarketMetrics {
        storage: compute_market_metric(&storage_state, true),
        compute: compute_market_metric(&compute_state, false),
        energy: compute_market_metric(&energy_state, false),
        ad: compute_ad_metric(&ad_state),
    }
}

/// Market state accumulated during epoch window scan.
struct MarketState {
    /// Total settlement revenue in CT
    revenue_ct: u64,
    /// Total capacity (bytes, compute units, energy units, impressions)
    capacity: u64,
    /// Provider escrow (for storage margin calculation)
    provider_escrow: u64,
    /// Conversion events (for ad market)
    conversions: u64,
    /// Impression count (for ad market)
    impressions: u64,
    /// Number of settlements
    settlement_count: u64,
    /// Number of verified computations
    verified_count: u64,
    /// Unique providers
    providers: HashSet<String>,
}

impl Default for MarketState {
    fn default() -> Self {
        Self {
            revenue_ct: 0,
            capacity: 0,
            provider_escrow: 0,
            conversions: 0,
            impressions: 0,
            settlement_count: 0,
            verified_count: 0,
            providers: HashSet::new(),
        }
    }
}

/// Compute market metric (utilization and provider margin) for a market domain.
fn compute_market_metric(state: &MarketState, is_storage: bool) -> MarketMetric {
    // Utilization: capacity utilization ratio [0.0, 1.0]
    // For storage: bytes settled / total capacity available
    // For compute: successful verifications / job capacity
    // For energy: units delivered / capacity
    let utilization = if state.capacity > 0 {
        if is_storage {
            // Storage: revenue indicates utilized capacity
            (state.revenue_ct as f64 / state.capacity as f64).clamp(0.0, 1.0)
        } else {
            // Compute/Energy: direct capacity tracking
            (state.settlement_count as f64 / state.capacity as f64).clamp(0.0, 1.0)
        }
    } else {
        0.0
    };

    // Provider margin: revenue / escrow or revenue / costs as proxy
    let provider_margin = if is_storage && state.provider_escrow > 0 {
        // Storage: ROI = revenue / escrow
        (state.revenue_ct as f64 / state.provider_escrow as f64).clamp(0.0, 1.0)
    } else if state.revenue_ct > 0 {
        // Compute/Energy: use default margins if no escrow data
        // These will be refined as receipt structures mature
        match state.settlement_count {
            0 => 0.5,                                                             // Default 50%
            count => ((state.revenue_ct as f64 / count as f64) / 100.0).min(0.8), // Cap at 80%
        }
    } else {
        0.5 // Default 50% margin
    };

    // Estimate average costs and payouts based on revenue
    let average_cost_block = if state.settlement_count > 0 {
        (state.revenue_ct as f64 / state.settlement_count as f64) * (1.0 - provider_margin)
    } else {
        0.0
    };

    let effective_payout_block = if state.settlement_count > 0 {
        state.revenue_ct as f64 / state.settlement_count as f64
    } else {
        0.0
    };

    MarketMetric {
        utilization,
        average_cost_block,
        effective_payout_block,
        provider_margin,
    }
}

/// Compute ad market metric with conversion tracking.
fn compute_ad_metric(state: &MarketState) -> MarketMetric {
    // Utilization: conversion rate [0.0, 1.0]
    let utilization = if state.impressions > 0 {
        (state.conversions as f64 / state.impressions as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Publisher margin: spend per settlement
    let provider_margin = if state.settlement_count > 0 {
        // Ad margin = revenue / settlement count (simplified)
        // In practice: (spend - platform_costs) / spend
        let margin = (state.revenue_ct as f64 / state.settlement_count as f64) / 100.0;
        margin.min(0.8).max(0.2) // Clamp between 20% and 80%
    } else {
        0.6 // Default 60% for publishers
    };

    let average_cost_block = if state.settlement_count > 0 {
        (state.revenue_ct as f64 / state.settlement_count as f64) * (1.0 - provider_margin)
    } else {
        0.0
    };

    let effective_payout_block = if state.settlement_count > 0 {
        state.revenue_ct as f64 / state.settlement_count as f64
    } else {
        0.0
    };

    MarketMetric {
        utilization,
        average_cost_block,
        effective_payout_block,
        provider_margin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_metrics_computed_correctly() {
        let mut state = MarketState::default();
        state.revenue_ct = 1000;
        state.capacity = 10000;
        state.provider_escrow = 5000;
        state.settlement_count = 1;

        let metric = compute_market_metric(&state, true);
        // utilization = 1000 / 10000 = 0.1
        assert!((metric.utilization - 0.1).abs() < 0.001);
        // margin = 1000 / 5000 = 0.2
        assert!((metric.provider_margin - 0.2).abs() < 0.001);
    }

    #[test]
    fn compute_metrics_computed_correctly() {
        let mut state = MarketState::default();
        state.revenue_ct = 500;
        state.capacity = 1000;
        state.settlement_count = 10;
        state.verified_count = 9;

        let metric = compute_market_metric(&state, false);
        // utilization = 10 / 1000 = 0.01
        assert!((metric.utilization - 0.01).abs() < 0.001);
    }

    #[test]
    fn ad_conversion_rate_computed() {
        let mut state = MarketState::default();
        state.impressions = 10000;
        state.conversions = 100;
        state.revenue_ct = 500;
        state.settlement_count = 1;

        let metric = compute_ad_metric(&state);
        // conversion_rate = 100 / 10000 = 0.01
        assert!((metric.utilization - 0.01).abs() < 0.001);
    }

    #[test]
    fn zero_capacity_handles_gracefully() {
        let state = MarketState::default();
        let metric = compute_market_metric(&state, true);
        assert_eq!(metric.utilization, 0.0);
        assert_eq!(metric.provider_margin, 0.5); // Default
    }
}
