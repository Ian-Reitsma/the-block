//! Layer 4: Ad Market & Tariff Controllers
//!
//! **Ad Market Drift**: Automatically adjusts ad revenue splits (platform/user/publisher)
//! to converge toward governance targets. Instead of frozen 45/25/30, splits drift
//! based on measured actual shares.
//!
//! **Tariff Controller**: Adjusts non-KYC tariff bps to maintain target treasury
//! contribution percentage. If non-KYC volume changes, tariff auto-adjusts.

use super::{AdMarketSnapshot, TariffSnapshot};
use foundation_serialization::{Deserialize, Serialize};

/// Ad market split parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdMarketParams {
    /// Target platform take (bps)
    pub platform_take_target_bps: u16,

    /// Target user attention share (bps)
    pub user_share_target_bps: u16,

    /// Drift rate (how fast splits adjust per epoch)
    pub drift_rate: f64,
}

impl Default for AdMarketParams {
    fn default() -> Self {
        Self {
            platform_take_target_bps: 2800, // 28% (beat Google's 30%)
            user_share_target_bps: 2200,    // 22% (meaningful UBI)
            drift_rate: 0.01,               // 1% drift per epoch
        }
    }
}

/// Tariff parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TariffParams {
    /// Target treasury contribution from tariffs (bps of total treasury inflow)
    pub public_revenue_target_bps: u16,

    /// Drift rate (how fast tariff adjusts per epoch)
    pub drift_rate: f64,

    /// Minimum tariff (bps)
    pub tariff_min_bps: u16,

    /// Maximum tariff (bps)
    pub tariff_max_bps: u16,
}

impl Default for TariffParams {
    fn default() -> Self {
        Self {
            public_revenue_target_bps: 1000, // 10% of treasury inflow
            drift_rate: 0.05,                // 5% drift per epoch
            tariff_min_bps: 0,
            tariff_max_bps: 200, // 2% max
        }
    }
}

pub struct AdMarketDriftController {
    params: AdMarketParams,
}

impl AdMarketDriftController {
    pub fn new(params: AdMarketParams) -> Self {
        Self { params }
    }

    /// Compute next epoch's ad market splits
    ///
    /// Formula: T_{t+1} = T_t + k × (T_target - T_t)
    ///          U_{t+1} = U_t + k × (U_target - U_t)
    ///          P_{t+1} = 1 - T_{t+1} - U_{t+1}
    ///
    /// # Arguments
    /// * `total_ad_spend_block` - Total ad spend this epoch (for measuring actual splits)
    ///
    /// # Note
    /// In practice, you'd measure actual T and U from ad settlement records.
    /// For this initial implementation, we drift toward targets from governance.
    pub fn compute_next_splits(&self, _total_ad_spend_block: u64) -> AdMarketSnapshot {
        // For now, just return governance targets
        // In full implementation, measure actual splits from ad settlement
        // and apply drift: T_next = T_current + k × (T_target - T_current)

        let t_target_bps = self.params.platform_take_target_bps;
        let u_target_bps = self.params.user_share_target_bps;

        // Publisher gets remainder: P = 10000 - T - U
        let p_target_bps = 10_000u16
            .saturating_sub(t_target_bps)
            .saturating_sub(u_target_bps);

        AdMarketSnapshot {
            platform_take_bps: t_target_bps,
            user_share_bps: u_target_bps,
            publisher_share_bps: p_target_bps,
        }
    }

}

pub struct TariffController {
    params: TariffParams,
}

impl TariffController {
    pub fn new(params: TariffParams) -> Self {
        Self { params }
    }

    /// Compute next epoch's tariff
    ///
    /// Formula: R_needed = R_target × I_treasury
    ///          τ_implied = (R_needed / F_tariff) in bps
    ///          τ_{t+1} = τ_t + k × (τ_implied - τ_t)
    ///          τ_{t+1} = clamp(τ_{t+1}, τ_min, τ_max)
    ///
    /// # Arguments
    /// * `non_kyc_volume_block` - Total non-KYC transaction volume this epoch
    /// * `treasury_inflow_block` - Total treasury inflow this epoch
    /// * `current_tariff_bps` - Current tariff rate
    ///
    /// # Returns
    /// Updated tariff snapshot
    pub fn compute_next_tariff(
        &self,
        non_kyc_volume_block: u64,
        treasury_inflow_block: u64,
        current_tariff_bps: u16,
    ) -> TariffSnapshot {
        // Avoid division by zero
        if non_kyc_volume_block == 0 || treasury_inflow_block == 0 {
            return TariffSnapshot {
                tariff_bps: current_tariff_bps,
                non_kyc_volume_block,
                treasury_contribution_bps: 0,
            };
        }

        // Target revenue from tariffs: R_target × I_treasury
        let r_target_ratio = (self.params.public_revenue_target_bps as f64) / 10_000.0;
        let r_needed = (treasury_inflow_block as f64) * r_target_ratio;

        // Implied tariff: τ = R_needed / F_tariff
        let tau_implied = r_needed / (non_kyc_volume_block as f64);
        let tau_implied_bps = (tau_implied * 10_000.0).round() as u16;

        // Drift toward implied tariff
        let k = self.params.drift_rate;
        let tau_current = current_tariff_bps as f64;
        let tau_next = tau_current + k * (tau_implied_bps as f64 - tau_current);

        // Clamp to bounds
        let tau_next_bps =
            (tau_next.round() as u16).clamp(self.params.tariff_min_bps, self.params.tariff_max_bps);

        // Compute actual treasury contribution
        let actual_revenue = (non_kyc_volume_block as f64) * (tau_next_bps as f64) / 10_000.0;
        let contribution_ratio = if treasury_inflow_block > 0 {
            actual_revenue / (treasury_inflow_block as f64)
        } else {
            0.0
        };
        let contribution_bps = (contribution_ratio * 10_000.0).round() as u16;

        TariffSnapshot {
            tariff_bps: tau_next_bps,
            non_kyc_volume_block,
            treasury_contribution_bps: contribution_bps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ad_market_splits_sum_to_10000() {
        let params = AdMarketParams::default();
        let controller = AdMarketDriftController::new(params);

        let splits = controller.compute_next_splits(1_000_000);

        assert_eq!(
            splits.platform_take_bps as u32
                + splits.user_share_bps as u32
                + splits.publisher_share_bps as u32,
            10_000
        );
    }

    #[test]
    fn test_tariff_at_target() {
        // Tariff is already producing target revenue → should stay stable
        let params = TariffParams {
            public_revenue_target_bps: 1000, // Want 10% of treasury
            drift_rate: 0.05,
            tariff_min_bps: 0,
            tariff_max_bps: 200,
        };
        let controller = TariffController::new(params);

        // Treasury inflow = 1M BLOCK
        // Want 10% from tariffs = 100k BLOCK
        // Non-KYC volume = 2M BLOCK
        // Current tariff = 50 bps = 0.5%
        // Actual revenue = 2M × 0.005 = 10k BLOCK (only 1% of treasury, not 10%)

        let treasury_inflow = 1_000_000u64;
        let non_kyc_volume = 2_000_000u64;
        let current_tariff = 50u16; // 0.5%

        let snapshot =
            controller.compute_next_tariff(non_kyc_volume, treasury_inflow, current_tariff);

        // Should drift up to increase revenue
        // Need 100k from 2M volume → 5% tariff (500 bps)
        // With 5% drift, should move from 50 toward 500
        assert!(snapshot.tariff_bps > current_tariff);
    }

    #[test]
    fn test_tariff_above_target() {
        // Tariff too low → should increase toward target
        let params = TariffParams {
            public_revenue_target_bps: 1000, // Want 10% of treasury
            drift_rate: 0.05,
            tariff_min_bps: 0,
            tariff_max_bps: 200,
        };
        let controller = TariffController::new(params);

        // Treasury inflow = 1M BLOCK
        // Want 10% from tariffs = 100k BLOCK
        // Non-KYC volume = 500k BLOCK
        // Current tariff = 50 bps = 0.5%
        // Implied tariff: 100k / 500k = 20% = 2000 bps (way above max)
        // With drift 0.05: 50 + 0.05 * (2000 - 50) = 50 + 97.5 = 147.5 ≈ 148 bps

        let treasury_inflow = 1_000_000u64;
        let non_kyc_volume = 500_000u64;
        let current_tariff = 50u16;

        let snapshot =
            controller.compute_next_tariff(non_kyc_volume, treasury_inflow, current_tariff);

        // Should drift up but not hit max yet (5% drift)
        assert!(snapshot.tariff_bps > current_tariff);
        assert!(snapshot.tariff_bps < 200); // Not at max yet
        assert_eq!(snapshot.tariff_bps, 148); // Calculated drift
    }

    #[test]
    fn test_tariff_bounds() {
        // Test min/max clamping
        let params = TariffParams {
            public_revenue_target_bps: 1000,
            drift_rate: 1.0, // Instant drift
            tariff_min_bps: 10,
            tariff_max_bps: 200,
        };
        let tariff_min = params.tariff_min_bps;
        let controller = TariffController::new(params);

        // Very high volume → would want near-zero tariff
        let treasury_inflow = 1_000_000u64;
        let non_kyc_volume = 100_000_000u64; // 100M volume
        let current_tariff = 100u16;

        let snapshot =
            controller.compute_next_tariff(non_kyc_volume, treasury_inflow, current_tariff);

        // Should clamp at min
        assert!(snapshot.tariff_bps >= tariff_min);
    }

    #[test]
    fn test_tariff_zero_volume() {
        // Zero volume → no crash, just return current
        let params = TariffParams::default();
        let controller = TariffController::new(params);

        let snapshot = controller.compute_next_tariff(0, 1_000_000, 50);

        assert_eq!(snapshot.tariff_bps, 50);
        assert_eq!(snapshot.non_kyc_volume_block, 0);
    }
}
