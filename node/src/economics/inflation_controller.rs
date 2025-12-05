//! Layer 1: Adaptive Global CT Issuance Controller
//!
//! Maintains inflation at target rate via proportional feedback control.
//! Formula: I_{t+1} = I_t × (1 + k_π × (π* - π_t))
//!
//! Instead of fixed 200M CT/year, issuance adapts to keep inflation stable
//! even if token price or adoption changes dramatically.

use super::{InflationSnapshot};
use foundation_serialization::{Deserialize, Serialize};

/// Inflation control parameters (all from governance)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct InflationParams {
    /// Target annual inflation in basis points (e.g., 500 = 5%)
    pub target_inflation_bps: u16,

    /// Proportional gain k_π (e.g., 0.10 = 10% of error per epoch)
    pub controller_gain: f64,

    /// Minimum annual issuance in CT (safety floor)
    pub min_annual_issuance_ct: u64,

    /// Maximum annual issuance in CT (safety ceiling)
    pub max_annual_issuance_ct: u64,

    /// Previous epoch's annual issuance (for continuity)
    pub previous_annual_issuance_ct: u64,
}

impl Default for InflationParams {
    fn default() -> Self {
        Self {
            target_inflation_bps: 500, // 5%
            controller_gain: 0.10,
            min_annual_issuance_ct: 50_000_000,
            max_annual_issuance_ct: 300_000_000,
            previous_annual_issuance_ct: 200_000_000,
        }
    }
}

pub struct InflationController {
    params: InflationParams,
}

impl InflationController {
    pub fn new(params: InflationParams) -> Self {
        Self { params }
    }

    /// Compute next epoch's issuance using proportional controller
    ///
    /// # Arguments
    /// * `circulating_ct` - Total CT in circulation at epoch start
    ///
    /// # Returns
    /// Updated inflation snapshot with new annual issuance
    pub fn compute_epoch_issuance(&self, circulating_ct: u64) -> InflationSnapshot {
        // Avoid division by zero
        if circulating_ct == 0 {
            return InflationSnapshot {
                circulating_ct: 0,
                annual_issuance_ct: self.params.min_annual_issuance_ct,
                realized_inflation_bps: 0,
                target_inflation_bps: self.params.target_inflation_bps,
            };
        }

        // Compute realized inflation: π_t = I_t / M_t
        let i_t = self.params.previous_annual_issuance_ct;
        let m_t = circulating_ct;

        let realized_inflation = (i_t as f64) / (m_t as f64);
        let realized_inflation_bps = (realized_inflation * 10_000.0).round() as u16;

        // Compute target inflation
        let pi_target = (self.params.target_inflation_bps as f64) / 10_000.0;

        // Proportional control law: I_{t+1} = I_t × (1 + k_π × (π* - π_t))
        let k_pi = self.params.controller_gain;
        let error = pi_target - realized_inflation;
        let adjustment_factor = 1.0 + (k_pi * error);

        let i_next_raw = (i_t as f64) * adjustment_factor;

        // Clamp to safety bounds
        let i_next = i_next_raw
            .max(self.params.min_annual_issuance_ct as f64)
            .min(self.params.max_annual_issuance_ct as f64)
            .round() as u64;

        InflationSnapshot {
            circulating_ct,
            annual_issuance_ct: i_next,
            realized_inflation_bps,
            target_inflation_bps: self.params.target_inflation_bps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inflation_controller_stable() {
        // At equilibrium (5% inflation), should maintain issuance
        let params = InflationParams {
            target_inflation_bps: 500, // 5%
            controller_gain: 0.10,
            min_annual_issuance_ct: 50_000_000,
            max_annual_issuance_ct: 300_000_000,
            previous_annual_issuance_ct: 200_000_000,
        };

        let controller = InflationController::new(params);

        // 200M / 4B = 5% inflation (at target)
        let circulating = 4_000_000_000u64;
        let snapshot = controller.compute_epoch_issuance(circulating);

        // Should stay near 200M (minimal adjustment)
        assert!(snapshot.annual_issuance_ct >= 199_000_000);
        assert!(snapshot.annual_issuance_ct <= 201_000_000);
        assert_eq!(snapshot.realized_inflation_bps, 500);
    }

    #[test]
    fn test_inflation_controller_below_target() {
        // Inflation too low (3%), should increase issuance
        let params = InflationParams {
            target_inflation_bps: 500, // 5%
            controller_gain: 0.10,
            min_annual_issuance_ct: 50_000_000,
            max_annual_issuance_ct: 300_000_000,
            previous_annual_issuance_ct: 200_000_000,
        };

        let controller = InflationController::new(params);

        // 200M / 6.67B ≈ 3% inflation (below target)
        let circulating = 6_666_666_666u64;
        let snapshot = controller.compute_epoch_issuance(circulating);

        // Should increase issuance to push inflation up
        assert!(snapshot.annual_issuance_ct > 200_000_000);
        assert_eq!(snapshot.realized_inflation_bps, 299); // ~3%
    }

    #[test]
    fn test_inflation_controller_above_target() {
        // Inflation too high (10%), should decrease issuance
        let params = InflationParams {
            target_inflation_bps: 500, // 5%
            controller_gain: 0.10,
            min_annual_issuance_ct: 50_000_000,
            max_annual_issuance_ct: 300_000_000,
            previous_annual_issuance_ct: 200_000_000,
        };

        let controller = InflationController::new(params);

        // 200M / 2B = 10% inflation (above target)
        let circulating = 2_000_000_000u64;
        let snapshot = controller.compute_epoch_issuance(circulating);

        // Should decrease issuance to push inflation down
        assert!(snapshot.annual_issuance_ct < 200_000_000);
        assert_eq!(snapshot.realized_inflation_bps, 1000); // 10%
    }

    #[test]
    fn test_inflation_controller_bounds() {
        // Test ceiling
        let params = InflationParams {
            target_inflation_bps: 500,
            controller_gain: 1.0, // Very aggressive
            min_annual_issuance_ct: 50_000_000,
            max_annual_issuance_ct: 300_000_000,
            previous_annual_issuance_ct: 200_000_000,
        };

        let controller = InflationController::new(params);

        // Very low circulation → would want massive issuance
        let circulating = 100_000_000u64;
        let snapshot = controller.compute_epoch_issuance(circulating);

        // Should clamp at ceiling
        assert_eq!(snapshot.annual_issuance_ct, 300_000_000);
    }

    #[test]
    fn test_inflation_controller_zero_circulation() {
        let params = InflationParams::default();
        let controller = InflationController::new(params);

        let snapshot = controller.compute_epoch_issuance(0);

        // Should return floor without division by zero
        assert_eq!(snapshot.annual_issuance_ct, params.min_annual_issuance_ct);
        assert_eq!(snapshot.realized_inflation_bps, 0);
    }
}
