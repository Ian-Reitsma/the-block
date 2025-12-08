//! Layer 3: Dual Multiplier Control
//!
//! Combines utilization-based AND cost-coverage multipliers for each market.
//!
//! Utilization multiplier: m^(U) = 1 + k_U × (U_target - U_actual)
//! Cost-coverage multiplier: m^(c) = 1 + k_c × ((c × (1 + m_target)) / p - 1)
//! Combined: M = clip(m^(U) × m^(c), M_min, M_max)
//!
//! This allows the system to respond to BOTH demand shocks (utilization changes)
//! AND supply shocks (cost changes like energy price spikes).

use super::{MarketMetrics, MultiplierSnapshot};
use foundation_serialization::{Deserialize, Serialize};

/// Multiplier control parameters per market
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MarketMultiplierParams {
    // Target utilization (bps)
    pub util_target_bps: u16,

    // Target provider margin (bps)
    pub margin_target_bps: u16,

    // Utilization responsiveness (k_U)
    pub util_responsiveness: f64,

    // Cost-coverage responsiveness (k_c)
    pub cost_responsiveness: f64,

    // Floor multiplier
    pub multiplier_floor: f64,

    // Ceiling multiplier
    pub multiplier_ceiling: f64,
}

impl Default for MarketMultiplierParams {
    fn default() -> Self {
        Self {
            util_target_bps: 5000, // 50%
            margin_target_bps: 3000, // 30%
            util_responsiveness: 2.0,
            cost_responsiveness: 1.0,
            multiplier_floor: 0.8,
            multiplier_ceiling: 3.0,
        }
    }
}

/// Complete multiplier parameters for all markets
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MultiplierParams {
    pub storage: MarketMultiplierParams,
    pub compute: MarketMultiplierParams,
    pub energy: MarketMultiplierParams,
    pub ad: MarketMultiplierParams,
}

impl Default for MultiplierParams {
    fn default() -> Self {
        Self {
            storage: MarketMultiplierParams {
                util_target_bps: 4000, // 40%
                margin_target_bps: 5000, // 50%
                util_responsiveness: 2.0,
                cost_responsiveness: 1.0,
                multiplier_floor: 0.8,
                multiplier_ceiling: 3.0,
            },
            compute: MarketMultiplierParams {
                util_target_bps: 6000, // 60%
                margin_target_bps: 5000, // 50%
                util_responsiveness: 2.0,
                cost_responsiveness: 1.0,
                multiplier_floor: 0.8,
                multiplier_ceiling: 3.0,
            },
            energy: MarketMultiplierParams {
                util_target_bps: 5000, // 50%
                margin_target_bps: 2500, // 25%
                util_responsiveness: 2.0,
                cost_responsiveness: 1.0,
                multiplier_floor: 0.8,
                multiplier_ceiling: 3.0,
            },
            ad: MarketMultiplierParams {
                util_target_bps: 5000, // 50%
                margin_target_bps: 3000, // 30%
                util_responsiveness: 2.0,
                cost_responsiveness: 1.0,
                multiplier_floor: 0.8,
                multiplier_ceiling: 3.0,
            },
        }
    }
}

pub struct MarketMultiplierController {
    params: MultiplierParams,
}

impl MarketMultiplierController {
    pub fn new(params: MultiplierParams) -> Self {
        Self { params }
    }

    /// Compute all market multipliers
    pub fn compute_multipliers(&self, metrics: &MarketMetrics) -> MultiplierSnapshot {
        MultiplierSnapshot {
            storage_multiplier: self.compute_market_multiplier(
                &metrics.storage,
                &self.params.storage,
            ),
            compute_multiplier: self.compute_market_multiplier(
                &metrics.compute,
                &self.params.compute,
            ),
            energy_multiplier: self.compute_market_multiplier(
                &metrics.energy,
                &self.params.energy,
            ),
            ad_multiplier: self.compute_market_multiplier(&metrics.ad, &self.params.ad),
        }
    }

    /// Compute dual multiplier for a single market
    fn compute_market_multiplier(
        &self,
        metric: &super::MarketMetric,
        params: &MarketMultiplierParams,
    ) -> f64 {
        // Utilization multiplier: m^(U) = 1 + k_U × (U_target - U_actual)
        let u_target = (params.util_target_bps as f64) / 10_000.0;
        let u_actual = metric.utilization.clamp(0.0, 1.0);
        let util_gap = u_target - u_actual;
        let m_u = 1.0 + params.util_responsiveness * util_gap;

        // Cost-coverage multiplier: m^(c) = 1 + k_c × ((c × (1 + m_target)) / p - 1)
        let m_c = if metric.effective_payout_block > 0.0 && metric.average_cost_block > 0.0 {
            let m_target = (params.margin_target_bps as f64) / 10_000.0;
            let cost_with_margin = metric.average_cost_block * (1.0 + m_target);
            let coverage_ratio = cost_with_margin / metric.effective_payout_block;
            let coverage_gap = coverage_ratio - 1.0;
            1.0 + params.cost_responsiveness * coverage_gap
        } else {
            // No cost data → use utilization only
            1.0
        };

        // Combined: M = m^(U) × m^(c)
        let m_combined = m_u * m_c;

        // Clamp to bounds
        m_combined.clamp(params.multiplier_floor, params.multiplier_ceiling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economics::MarketMetric;

    #[test]
    fn test_multiplier_at_target() {
        // Market at target → multiplier near 1.0
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40, // At target (40%)
                average_cost_block: 100.0,
                effective_payout_block: 150.0, // 50% margin (at target)
                provider_margin: 0.50,
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should be near 1.0 (within 10%)
        assert!(multipliers.storage_multiplier >= 0.90);
        assert!(multipliers.storage_multiplier <= 1.10);
    }

    #[test]
    fn test_multiplier_low_utilization() {
        // Low utilization → increase multiplier
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: 0.10, // Very low (target 40%)
                average_cost_block: 100.0,
                effective_payout_block: 150.0,
                provider_margin: 0.50,
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should be > 1.0 to attract more providers
        assert!(multipliers.storage_multiplier > 1.0);
    }

    #[test]
    fn test_multiplier_high_cost() {
        // Costs rise → increase multiplier to maintain margin
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            energy: MarketMetric {
                utilization: 0.50, // At target
                average_cost_block: 200.0, // High cost
                effective_payout_block: 150.0, // Below needed payout
                provider_margin: -0.15, // Unprofitable
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should be > 1.0 to compensate for high costs
        assert!(multipliers.energy_multiplier > 1.0);
    }

    #[test]
    fn test_multiplier_overhealthy() {
        // Over-utilized and over-profitable → decrease multiplier
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            compute: MarketMetric {
                utilization: 0.90, // Very high (target 60%)
                average_cost_block: 100.0,
                effective_payout_block: 300.0, // Very profitable
                provider_margin: 2.0, // 200% margin
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should be < 1.0 to reduce overpayment
        assert!(multipliers.compute_multiplier < 1.0);
    }

    #[test]
    fn test_multiplier_bounds() {
        // Extreme conditions → should clamp at bounds
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            energy: MarketMetric {
                utilization: 0.0, // Zero utilization
                average_cost_block: 1000.0, // Very high cost
                effective_payout_block: 10.0, // Very low payout
                provider_margin: -10.0, // Massively unprofitable
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should clamp at ceiling (3.0)
        assert_eq!(multipliers.energy_multiplier, 3.0);
    }

    #[test]
    fn test_multiplier_no_cost_data() {
        // No cost data → use utilization only
        let params = MultiplierParams::default();
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            ad: MarketMetric {
                utilization: 0.20, // Below target
                average_cost_block: 0.0, // No cost data
                effective_payout_block: 0.0, // No payout data
                provider_margin: 0.0,
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Should still work (utilization-based only)
        assert!(multipliers.ad_multiplier > 1.0);
    }

    #[test]
    fn test_multiplier_dual_effect() {
        // Both utilization low AND costs high → multiplier amplified
        let params = MultiplierParams {
            energy: MarketMultiplierParams {
                util_target_bps: 5000,
                margin_target_bps: 2500,
                util_responsiveness: 2.0,
                cost_responsiveness: 1.0,
                multiplier_floor: 0.8,
                multiplier_ceiling: 3.0,
            },
            ..Default::default()
        };
        let controller = MarketMultiplierController::new(params);

        let metrics = MarketMetrics {
            energy: MarketMetric {
                utilization: 0.20, // Low (target 50%)
                average_cost_block: 200.0,
                effective_payout_block: 150.0, // Below needed
                provider_margin: -0.15,
            },
            ..Default::default()
        };

        let multipliers = controller.compute_multipliers(&metrics);

        // Both effects should multiply
        // m^(U) ≈ 1 + 2.0 × (0.5 - 0.2) = 1.6
        // m^(c) ≈ 1 + 1.0 × (250/150 - 1) ≈ 1.67
        // Combined ≈ 1.6 × 1.67 ≈ 2.67
        assert!(multipliers.energy_multiplier > 2.0);
        assert!(multipliers.energy_multiplier <= 3.0); // Clamped at ceiling
    }
}
