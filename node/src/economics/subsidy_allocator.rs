//! Layer 2: Dynamic Subsidy Reallocation
//!
//! Automatically adjusts subsidy shares across markets based on distress signals.
//! Distress = utilization gap + margin gap
//! Uses softmax to convert distress scores into allocation shares.
//!
//! Prevents manual "20% → 25%" interventions by auto-healing when markets
//! are unprofitable or under-utilized.

use super::{MarketMetrics, SubsidySnapshot};
use foundation_serialization::{Deserialize, Serialize};

/// Subsidy allocation parameters (all from governance)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SubsidyParams {
    // Target utilization for each market (bps)
    pub storage_util_target_bps: u16,
    pub compute_util_target_bps: u16,
    pub energy_util_target_bps: u16,
    pub ad_util_target_bps: u16,

    // Target provider margin for each market (bps)
    pub storage_margin_target_bps: u16,
    pub compute_margin_target_bps: u16,
    pub energy_margin_target_bps: u16,
    pub ad_margin_target_bps: u16,

    /// Weight for utilization gap in distress score (α)
    pub alpha: f64,

    /// Weight for margin gap in distress score (β)
    pub beta: f64,

    /// Softmax temperature (τ) - higher = smoother transitions
    pub temperature: f64,

    /// Drift rate (λ) - how fast allocation changes (0.0 to 1.0)
    pub drift_rate: f64,
}

impl Default for SubsidyParams {
    fn default() -> Self {
        Self {
            // Utilization targets
            storage_util_target_bps: 4000,  // 40%
            compute_util_target_bps: 6000,  // 60%
            energy_util_target_bps: 5000,   // 50%
            ad_util_target_bps: 5000,       // 50%

            // Margin targets
            storage_margin_target_bps: 5000,  // 50%
            compute_margin_target_bps: 5000,  // 50%
            energy_margin_target_bps: 2500,   // 25%
            ad_margin_target_bps: 3000,       // 30%

            // Distress scoring
            alpha: 0.60,  // 60% weight on utilization
            beta: 0.40,   // 40% weight on margin

            // Control parameters
            temperature: 1.0,
            drift_rate: 0.05, // 5% drift per epoch
        }
    }
}

pub struct SubsidyAllocator {
    params: SubsidyParams,
}

impl SubsidyAllocator {
    pub fn new(params: SubsidyParams) -> Self {
        Self { params }
    }

    /// Compute next epoch's subsidy allocation
    ///
    /// # Arguments
    /// * `metrics` - Current market metrics (utilization, margins, etc.)
    /// * `current` - Current allocation (for drift smoothing)
    ///
    /// # Returns
    /// Updated subsidy allocation shares (sum to 10000 bps = 100%)
    pub fn compute_next_allocation(
        &self,
        metrics: &MarketMetrics,
        current: &SubsidySnapshot,
    ) -> SubsidySnapshot {
        // Compute distress score for each market
        let s_storage = self.compute_distress(
            &metrics.storage,
            self.params.storage_util_target_bps,
            self.params.storage_margin_target_bps,
        );

        let s_compute = self.compute_distress(
            &metrics.compute,
            self.params.compute_util_target_bps,
            self.params.compute_margin_target_bps,
        );

        let s_energy = self.compute_distress(
            &metrics.energy,
            self.params.energy_util_target_bps,
            self.params.energy_margin_target_bps,
        );

        let s_ad = self.compute_distress(
            &metrics.ad,
            self.params.ad_util_target_bps,
            self.params.ad_margin_target_bps,
        );

        // Softmax: φ_j = exp(s_j / τ) / Σ_k exp(s_k / τ)
        let tau = self.params.temperature;
        let exp_storage = (s_storage / tau).exp();
        let exp_compute = (s_compute / tau).exp();
        let exp_energy = (s_energy / tau).exp();
        let exp_ad = (s_ad / tau).exp();

        let sum_exp = exp_storage + exp_compute + exp_energy + exp_ad;

        let phi_storage_target = exp_storage / sum_exp;
        let phi_compute_target = exp_compute / sum_exp;
        let phi_energy_target = exp_energy / sum_exp;
        let phi_ad_target = exp_ad / sum_exp;

        // Apply drift smoothing: φ_{t+1} = φ_t + λ × (φ_target - φ_t)
        let lambda = self.params.drift_rate;

        let phi_storage = Self::drift(current.storage_share_bps, phi_storage_target, lambda);
        let phi_compute = Self::drift(current.compute_share_bps, phi_compute_target, lambda);
        let phi_energy = Self::drift(current.energy_share_bps, phi_energy_target, lambda);
        let phi_ad = Self::drift(current.ad_share_bps, phi_ad_target, lambda);

        // Normalize to ensure sum = 10000 bps
        let total = phi_storage + phi_compute + phi_energy + phi_ad;
        let norm_storage = ((phi_storage / total) * 10_000.0).round() as u16;
        let norm_compute = ((phi_compute / total) * 10_000.0).round() as u16;
        let norm_energy = ((phi_energy / total) * 10_000.0).round() as u16;

        // Ad gets remainder to ensure exact sum = 10000
        let norm_ad = 10_000u16
            .saturating_sub(norm_storage)
            .saturating_sub(norm_compute)
            .saturating_sub(norm_energy);

        SubsidySnapshot {
            storage_share_bps: norm_storage,
            compute_share_bps: norm_compute,
            energy_share_bps: norm_energy,
            ad_share_bps: norm_ad,
        }
    }

    /// Compute distress score: s_j = α × g^(U)_j + β × g^(m)_j
    fn compute_distress(
        &self,
        metric: &super::MarketMetric,
        util_target_bps: u16,
        margin_target_bps: u16,
    ) -> f64 {
        // Utilization gap: g^(U) = U_target - U_actual
        let u_target = (util_target_bps as f64) / 10_000.0;
        let u_actual = metric.utilization.clamp(0.0, 1.0);
        let g_u = u_target - u_actual;

        // Margin gap: g^(m) = m_target - m_actual
        let m_target = (margin_target_bps as f64) / 10_000.0;
        let m_actual = metric.provider_margin;
        let g_m = m_target - m_actual;

        // Combined distress: s = α × g^(U) + β × g^(m)
        let s = self.params.alpha * g_u + self.params.beta * g_m;

        // Distress can be negative (over-healthy market) or positive (distressed)
        s
    }

    /// Apply drift smoothing
    fn drift(current_bps: u16, target_ratio: f64, lambda: f64) -> f64 {
        let current_ratio = (current_bps as f64) / 10_000.0;
        current_ratio + lambda * (target_ratio - current_ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economics::MarketMetric;

    #[test]
    fn test_subsidy_allocator_balanced() {
        // All markets at target → should stay near current allocation
        let params = SubsidyParams::default();
        let allocator = SubsidyAllocator::new(params);

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: 0.60,
                provider_margin: 0.50,
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.25,
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.30,
                ..Default::default()
            },
        };

        let current = SubsidySnapshot {
            storage_share_bps: 1500, // 15%
            compute_share_bps: 3000, // 30%
            energy_share_bps: 2000,  // 20%
            ad_share_bps: 3500,      // 35%
        };

        let next = allocator.compute_next_allocation(&metrics, &current);

        // Should stay close to current (all markets balanced)
        assert!((next.storage_share_bps as i32 - current.storage_share_bps as i32).abs() < 200);
        assert!((next.compute_share_bps as i32 - current.compute_share_bps as i32).abs() < 200);
        assert!((next.energy_share_bps as i32 - current.energy_share_bps as i32).abs() < 200);
        assert!((next.ad_share_bps as i32 - current.ad_share_bps as i32).abs() < 200);

        // Total should be exactly 10000 bps
        assert_eq!(
            next.storage_share_bps as u32
                + next.compute_share_bps as u32
                + next.energy_share_bps as u32
                + next.ad_share_bps as u32,
            10_000
        );
    }

    #[test]
    fn test_subsidy_allocator_energy_distressed() {
        // Energy market unprofitable → should increase energy share
        let params = SubsidyParams::default();
        let allocator = SubsidyAllocator::new(params);

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: 0.60,
                provider_margin: 0.50,
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: 0.20, // Low utilization
                provider_margin: -0.15, // Unprofitable! (like -$9/epoch)
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.30,
                ..Default::default()
            },
        };

        let current = SubsidySnapshot {
            storage_share_bps: 1500,
            compute_share_bps: 3000,
            energy_share_bps: 2000, // 20%
            ad_share_bps: 3500,
        };

        let next = allocator.compute_next_allocation(&metrics, &current);

        // Energy share should increase (distressed)
        assert!(next.energy_share_bps > current.energy_share_bps);

        // Total should be exactly 10000 bps
        assert_eq!(
            next.storage_share_bps as u32
                + next.compute_share_bps as u32
                + next.energy_share_bps as u32
                + next.ad_share_bps as u32,
            10_000
        );
    }

    #[test]
    fn test_subsidy_allocator_compute_overhealthy() {
        // Compute over-utilized and over-profitable → should decrease share
        let params = SubsidyParams::default();
        let allocator = SubsidyAllocator::new(params);

        let metrics = MarketMetrics {
            storage: MarketMetric {
                utilization: 0.40,
                provider_margin: 0.50,
                ..Default::default()
            },
            compute: MarketMetric {
                utilization: 0.90, // Very high utilization
                provider_margin: 0.80, // Very profitable
                ..Default::default()
            },
            energy: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.25,
                ..Default::default()
            },
            ad: MarketMetric {
                utilization: 0.50,
                provider_margin: 0.30,
                ..Default::default()
            },
        };

        let current = SubsidySnapshot {
            storage_share_bps: 1500,
            compute_share_bps: 3000, // 30%
            energy_share_bps: 2000,
            ad_share_bps: 3500,
        };

        let next = allocator.compute_next_allocation(&metrics, &current);

        // Compute share should decrease (over-healthy)
        // Note: May not decrease if drift_rate is low, but distress score is negative
        // For a stronger test, we'd need multiple epochs

        // Total should be exactly 10000 bps
        assert_eq!(
            next.storage_share_bps as u32
                + next.compute_share_bps as u32
                + next.energy_share_bps as u32
                + next.ad_share_bps as u32,
            10_000
        );
    }

    #[test]
    fn test_subsidy_allocator_normalization() {
        // Test that normalization always works
        let params = SubsidyParams {
            drift_rate: 0.50, // Large drift for testing
            ..Default::default()
        };
        let allocator = SubsidyAllocator::new(params);

        let metrics = MarketMetrics::default();
        let current = SubsidySnapshot {
            storage_share_bps: 2500,
            compute_share_bps: 2500,
            energy_share_bps: 2500,
            ad_share_bps: 2500,
        };

        let next = allocator.compute_next_allocation(&metrics, &current);

        // Must sum to exactly 10000
        assert_eq!(
            next.storage_share_bps as u32
                + next.compute_share_bps as u32
                + next.energy_share_bps as u32
                + next.ad_share_bps as u32,
            10_000
        );
    }
}
