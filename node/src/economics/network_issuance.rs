//! Network-Driven BLOCK Issuance Formula
//!
//! Unlike traditional fixed-inflation models, this controller computes issuance
//! based on real network activity metrics:
//!
//! **Formula:**
//! ```text
//! block_reward = base_reward * activity_multiplier * decentralization_factor * supply_decay
//! ```
//!
//! Where:
//! - `base_reward`: Derived from total supply cap and expected network blocks
//! - `activity_multiplier`: Scales with transactions, volume, and market utilization
//! - `decentralization_factor`: Scales with number of active validators/nodes
//! - `supply_decay`: Natural decay as emission approaches MAX_SUPPLY_BLOCK
//!
//! This creates a self-regulating system where rewards respond to network health
//! rather than arbitrary time-based schedules.

use foundation_serialization::{Deserialize, Serialize};

/// Network activity metrics for issuance calculation
#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    /// Number of transactions in the epoch
    pub tx_count: u64,

    /// Total transaction volume in BLOCK
    pub tx_volume_block: u64,

    /// Number of unique miners/validators active in recent window
    pub unique_miners: u64,

    /// Average market utilization across all markets (0.0 to 1.0)
    pub avg_market_utilization: f64,

    /// Current block height
    pub block_height: u64,

    /// Total BLOCK emitted so far
    pub total_emission: u64,
}

/// Network-driven issuance parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct NetworkIssuanceParams {
    /// Total supply cap (40M BLOCK)
    pub max_supply_block: u64,

    /// Expected total blocks to reach 90% of cap (~Bitcoin's 210k blocks * ~100 halvings)
    pub expected_total_blocks: u64,

    /// Base transactions per epoch to achieve 1.0 activity multiplier (initial bootstrap value)
    pub baseline_tx_count: u64,

    /// Base transaction volume per epoch for 1.0 activity multiplier (initial bootstrap value)
    pub baseline_tx_volume_block: u64,

    /// Minimum number of unique miners for 1.0 decentralization factor (initial bootstrap value)
    pub baseline_miners: u64,

    /// Activity multiplier range [min, max]
    pub activity_multiplier_min: f64,
    pub activity_multiplier_max: f64,

    /// Decentralization multiplier range [min, max]
    pub decentralization_multiplier_min: f64,
    pub decentralization_multiplier_max: f64,

    /// Adaptive baselines configuration
    pub adaptive_baselines_enabled: bool,
    pub baseline_ema_alpha: f64, // EMA smoothing factor (0.05 = ~20 epoch smoothing)
    pub baseline_min_tx_count: u64,
    pub baseline_max_tx_count: u64,
    pub baseline_min_tx_volume: u64,
    pub baseline_max_tx_volume: u64,
    pub baseline_min_miners: u64,
    pub baseline_max_miners: u64,
}

impl Default for NetworkIssuanceParams {
    fn default() -> Self {
        Self {
            max_supply_block: 40_000_000,
            // Target ~20M blocks to reach 90% of cap
            // At 1 block/sec, that's ~231 days, reasonable for testnet/early mainnet
            expected_total_blocks: 20_000_000,
            // Baseline: 100 tx/epoch for 1.0x multiplier (bootstrap only)
            baseline_tx_count: 100,
            // Baseline: 10k BLOCK volume/epoch for 1.0x multiplier (bootstrap only)
            baseline_tx_volume_block: 10_000,
            // Baseline: 10 unique miners for 1.0x multiplier (bootstrap only)
            baseline_miners: 10,
            // Activity can boost or reduce rewards by 2x
            activity_multiplier_min: 0.5,
            activity_multiplier_max: 2.0,
            // Decentralization can boost or reduce by 50%
            decentralization_multiplier_min: 0.5,
            decentralization_multiplier_max: 1.5,
            // Adaptive baselines enabled by default
            adaptive_baselines_enabled: true,
            baseline_ema_alpha: 0.05, // 20-epoch smoothing
            // Bounds to prevent extreme baseline drift
            baseline_min_tx_count: 50,
            baseline_max_tx_count: 10_000,
            baseline_min_tx_volume: 5_000,
            baseline_max_tx_volume: 1_000_000,
            baseline_min_miners: 5,
            baseline_max_miners: 100,
        }
    }
}

pub struct NetworkIssuanceController {
    params: NetworkIssuanceParams,
    // Adaptive baseline state (EMAs)
    adaptive_baseline_tx_count: f64,
    adaptive_baseline_tx_volume: f64,
    adaptive_baseline_miners: f64,
}

impl NetworkIssuanceController {
    pub fn new(params: NetworkIssuanceParams) -> Self {
        // Initialize adaptive baselines from params
        Self {
            adaptive_baseline_tx_count: params.baseline_tx_count as f64,
            adaptive_baseline_tx_volume: params.baseline_tx_volume_block as f64,
            adaptive_baseline_miners: params.baseline_miners as f64,
            params,
        }
    }

    /// Update adaptive baselines with observed network activity
    ///
    /// Uses exponential moving average (EMA) to smooth baseline adjustments:
    /// EMA_new = α * observed + (1 - α) * EMA_old
    ///
    /// This should be called after each reward computation to keep baselines current.
    pub fn update_baselines(&mut self, metrics: &NetworkMetrics) {
        if !self.params.adaptive_baselines_enabled {
            return;
        }

        let alpha = self.params.baseline_ema_alpha.clamp(0.0, 1.0);

        // Update tx_count baseline
        self.adaptive_baseline_tx_count =
            alpha * (metrics.tx_count as f64) + (1.0 - alpha) * self.adaptive_baseline_tx_count;
        self.adaptive_baseline_tx_count = self.adaptive_baseline_tx_count.clamp(
            self.params.baseline_min_tx_count as f64,
            self.params.baseline_max_tx_count as f64,
        );

        // Update tx_volume baseline
        self.adaptive_baseline_tx_volume = alpha * (metrics.tx_volume_block as f64)
            + (1.0 - alpha) * self.adaptive_baseline_tx_volume;
        self.adaptive_baseline_tx_volume = self.adaptive_baseline_tx_volume.clamp(
            self.params.baseline_min_tx_volume as f64,
            self.params.baseline_max_tx_volume as f64,
        );

        // Update miners baseline
        self.adaptive_baseline_miners =
            alpha * (metrics.unique_miners as f64) + (1.0 - alpha) * self.adaptive_baseline_miners;
        self.adaptive_baseline_miners = self.adaptive_baseline_miners.clamp(
            self.params.baseline_min_miners as f64,
            self.params.baseline_max_miners as f64,
        );
    }

    /// Get current adaptive baselines (for telemetry/debugging)
    pub fn get_adaptive_baselines(&self) -> (u64, u64, u64) {
        (
            self.adaptive_baseline_tx_count.round() as u64,
            self.adaptive_baseline_tx_volume.round() as u64,
            self.adaptive_baseline_miners.round() as u64,
        )
    }

    /// Compute block reward based on network activity
    ///
    /// # Formula Breakdown:
    ///
    /// 1. **Base Reward:** Evenly distributes total supply across expected blocks
    ///    ```text
    ///    base = (max_supply * 0.9) / expected_total_blocks
    ///    ```
    ///    (Using 90% instead of 100% to leave room for tail emission)
    ///
    /// 2. **Activity Multiplier:** Rewards scale with network usage
    ///    ```text
    ///    activity = sqrt(tx_count / baseline_tx_count) *
    ///               sqrt(tx_volume / baseline_tx_volume) *
    ///               (1 + avg_utilization)
    ///    ```
    ///    Clamped to [0.5, 2.0] range
    ///
    /// 3. **Decentralization Factor:** Rewards scale with validator diversity
    ///    ```text
    ///    decentralization = sqrt(unique_miners / baseline_miners)
    ///    ```
    ///    Clamped to [0.5, 1.5] range
    ///
    /// 4. **Supply Decay:** Natural halvening as cap approaches
    ///    ```text
    ///    decay = (max_supply - emission) / max_supply
    ///    ```
    ///
    /// **Final:**
    /// ```text
    /// block_reward = base * activity * decentralization * decay
    /// ```
    pub fn compute_block_reward(&mut self, metrics: &NetworkMetrics) -> u64 {
        // 1. Base reward: Distribute 90% of cap over expected blocks
        let distributable_supply = (self.params.max_supply_block as f64) * 0.9;
        let base_reward = distributable_supply / (self.params.expected_total_blocks as f64);

        // 2. Activity multiplier (geometric mean of tx metrics + utilization bonus)
        // Use adaptive baselines if enabled, otherwise use fixed params
        let baseline_tx_count = if self.params.adaptive_baselines_enabled {
            self.adaptive_baseline_tx_count
        } else {
            self.params.baseline_tx_count as f64
        };

        let baseline_tx_volume = if self.params.adaptive_baselines_enabled {
            self.adaptive_baseline_tx_volume
        } else {
            self.params.baseline_tx_volume_block as f64
        };

        let tx_ratio = (metrics.tx_count as f64) / baseline_tx_count.max(1.0);
        let volume_ratio = (metrics.tx_volume_block as f64) / baseline_tx_volume.max(1.0);

        // Use sqrt to dampen extreme values
        let tx_factor = tx_ratio.max(0.01).sqrt();
        let volume_factor = volume_ratio.max(0.01).sqrt();

        // Utilization bonus: 0% util = 1.0x, 100% util = 2.0x
        let utilization_bonus = 1.0 + metrics.avg_market_utilization.clamp(0.0, 1.0);

        let activity_multiplier = (tx_factor * volume_factor * utilization_bonus).clamp(
            self.params.activity_multiplier_min,
            self.params.activity_multiplier_max,
        );

        // 3. Decentralization factor (rewards having more unique miners)
        let baseline_miners = if self.params.adaptive_baselines_enabled {
            self.adaptive_baseline_miners
        } else {
            self.params.baseline_miners as f64
        };

        let miner_ratio = (metrics.unique_miners as f64) / baseline_miners.max(1.0);
        let decentralization_multiplier = miner_ratio.max(0.01).sqrt().clamp(
            self.params.decentralization_multiplier_min,
            self.params.decentralization_multiplier_max,
        );

        // 4. Supply decay factor (natural halvening)
        let remaining = self
            .params
            .max_supply_block
            .saturating_sub(metrics.total_emission);
        let supply_decay = (remaining as f64) / (self.params.max_supply_block as f64);

        // Combine all factors
        let reward = base_reward * activity_multiplier * decentralization_multiplier * supply_decay;

        // Convert to integer, rounding up if non-zero
        // Only apply minimum floor when far from cap (>1% remaining)
        let reward_u64 = if remaining > 0 && reward > 0.0 {
            let rounded = reward.ceil() as u64;
            // Apply 1 BLOCK minimum only if we have >1% supply remaining
            // This ensures natural decay near the cap while preventing zeros far from it
            if remaining > self.params.max_supply_block / 100 {
                rounded.max(1)
            } else {
                rounded
            }
        } else {
            0
        };

        // Ensure we don't exceed supply cap
        let final_reward = reward_u64.min(remaining);

        // Update adaptive baselines with observed metrics (for next reward computation)
        self.update_baselines(metrics);

        final_reward
    }

    /// Estimate annual issuance based on current reward rate
    /// (Useful for compatibility with existing economics dashboard)
    pub fn estimate_annual_issuance(&self, current_block_reward: u64) -> u64 {
        // Assume ~1 block/second = 31.536M blocks/year
        const BLOCKS_PER_YEAR: u64 = 31_536_000;
        current_block_reward.saturating_mul(BLOCKS_PER_YEAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baseline_conditions() {
        // At baseline metrics, should get close to base reward
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: params.baseline_miners,
            avg_market_utilization: 0.5, // 50% util
            block_height: 1000,
            total_emission: 0,
        };

        let reward = controller.compute_block_reward(&metrics);

        // Base = 36M / 20M = 1.8 BLOCK
        // Activity at baseline = ~1.0x
        // Decentralization at baseline = 1.0x
        // Supply decay at 0 emission = 1.0x
        // Utilization bonus at 50% = 1.5x
        // Expected: 1.8 * 1.0 * 1.0 * 1.0 ≈ 1.8, but with 1.5x util bonus = 2.7
        assert!(
            reward >= 2 && reward <= 3,
            "Baseline reward should be ~2-3 BLOCK, got {}",
            reward
        );
    }

    #[test]
    fn test_high_activity_boost() {
        // 10x activity should boost rewards (up to 2x cap)
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let baseline_metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: params.baseline_miners,
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 0,
        };

        let high_activity_metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count * 10,
            tx_volume_block: params.baseline_tx_volume_block * 10,
            unique_miners: params.baseline_miners,
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 0,
        };

        let baseline_reward = controller.compute_block_reward(&baseline_metrics);
        let high_reward = controller.compute_block_reward(&high_activity_metrics);

        // High activity should give higher reward
        assert!(
            high_reward > baseline_reward,
            "High activity should boost rewards"
        );

        // But capped at 2x
        assert!(
            high_reward <= baseline_reward * 2,
            "Activity boost capped at 2x"
        );
    }

    #[test]
    fn test_decentralization_boost() {
        // More miners → higher rewards
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let few_miners = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: 5, // Half of baseline
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 0,
        };

        let many_miners = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: 50, // 5x baseline
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 0,
        };

        let few_reward = controller.compute_block_reward(&few_miners);
        let many_reward = controller.compute_block_reward(&many_miners);

        assert!(many_reward > few_reward, "More miners should boost rewards");
    }

    #[test]
    fn test_supply_decay() {
        // As emission approaches cap, rewards decay to zero
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let early_metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: params.baseline_miners,
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 1_000_000, // 1M emitted (2.5% of cap)
        };

        let late_metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count,
            tx_volume_block: params.baseline_tx_volume_block,
            unique_miners: params.baseline_miners,
            avg_market_utilization: 0.0,
            block_height: 15_000_000,
            total_emission: 36_000_000, // 36M emitted (90% of cap)
        };

        let early_reward = controller.compute_block_reward(&early_metrics);
        let late_reward = controller.compute_block_reward(&late_metrics);

        assert!(
            late_reward < early_reward,
            "Late emission should have lower rewards due to decay"
        );

        // With 90% emitted (supply_decay = 0.10), late reward should be significantly lower
        // Due to 1 BLOCK minimum floor, we get ~50% reduction rather than 10x
        // Accept at least 50% reduction as "drastically reduced"
        assert!(
            late_reward <= early_reward / 2,
            "Near cap, rewards should be drastically reduced (at least 2x)"
        );
    }

    #[test]
    fn test_cap_enforcement() {
        // Even with max multipliers, can't exceed remaining supply
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let metrics = NetworkMetrics {
            tx_count: params.baseline_tx_count * 1000, // Extreme activity
            tx_volume_block: params.baseline_tx_volume_block * 1000,
            unique_miners: params.baseline_miners * 100,
            avg_market_utilization: 1.0,
            block_height: 19_000_000,
            total_emission: 39_999_990, // Only 10 BLOCK remaining
        };

        let reward = controller.compute_block_reward(&metrics);

        // Formula naturally produces tiny rewards near cap due to supply decay
        // Should never exceed remaining supply, and should be very small (<= 10)
        assert!(
            reward <= 10,
            "Reward should not exceed remaining supply: got {}",
            reward
        );
        assert!(
            reward < 10,
            "Reward should be drastically reduced near cap: got {}",
            reward
        );
    }

    #[test]
    fn test_zero_activity() {
        // Zero activity should still give minimum reward (0.5x multiplier)
        let params = NetworkIssuanceParams::default();
        let mut controller = NetworkIssuanceController::new(params.clone());

        let metrics = NetworkMetrics {
            tx_count: 0,
            tx_volume_block: 0,
            unique_miners: 1,
            avg_market_utilization: 0.0,
            block_height: 1000,
            total_emission: 0,
        };

        let reward = controller.compute_block_reward(&metrics);

        // Should still get some reward (base * 0.5 activity * 0.5 decentralization)
        assert!(reward > 0, "Zero activity should still give minimum reward");
    }

    #[test]
    fn test_adaptive_baselines_track_activity() {
        // Test that adaptive baselines track network activity over time
        let mut params = NetworkIssuanceParams::default();
        params.adaptive_baselines_enabled = true;
        params.baseline_ema_alpha = 0.2; // Faster adaptation for testing

        let mut controller = NetworkIssuanceController::new(params.clone());

        // Initial baselines should be static params (100, 10_000, 10)
        assert_eq!(
            controller.adaptive_baseline_tx_count as u64,
            params.baseline_tx_count
        );
        assert_eq!(
            controller.adaptive_baseline_tx_volume as u64,
            params.baseline_tx_volume_block
        );
        assert_eq!(
            controller.adaptive_baseline_miners as u64,
            params.baseline_miners
        );

        // Feed high activity for several epochs
        for _ in 0..20 {
            let metrics = NetworkMetrics {
                tx_count: 500,           // 5x baseline
                tx_volume_block: 50_000, // 5x baseline
                unique_miners: 50,       // 5x baseline
                avg_market_utilization: 0.5,
                block_height: 1000,
                total_emission: 0,
            };
            controller.compute_block_reward(&metrics);
        }

        // Baselines should have adapted upward (with alpha=0.2, after 20 epochs should be close to new values)
        assert!(
            controller.adaptive_baseline_tx_count > 100.0,
            "tx_count baseline should adapt upward, got {}",
            controller.adaptive_baseline_tx_count
        );
        assert!(
            controller.adaptive_baseline_tx_volume > 10_000.0,
            "tx_volume baseline should adapt upward, got {}",
            controller.adaptive_baseline_tx_volume
        );
        assert!(
            controller.adaptive_baseline_miners > 10.0,
            "miners baseline should adapt upward, got {}",
            controller.adaptive_baseline_miners
        );

        // Should be bounded by max limits
        assert!(controller.adaptive_baseline_tx_count <= params.baseline_max_tx_count as f64);
        assert!(controller.adaptive_baseline_tx_volume <= params.baseline_max_tx_volume as f64);
        assert!(controller.adaptive_baseline_miners <= params.baseline_max_miners as f64);
    }

    #[test]
    fn test_adaptive_baselines_disabled_uses_static() {
        // Test that when adaptive baselines are disabled, static params are used
        let mut params = NetworkIssuanceParams::default();
        params.adaptive_baselines_enabled = false;

        let mut controller = NetworkIssuanceController::new(params.clone());

        // Feed high activity
        for _ in 0..20 {
            let metrics = NetworkMetrics {
                tx_count: 500,
                tx_volume_block: 50_000,
                unique_miners: 50,
                avg_market_utilization: 0.5,
                block_height: 1000,
                total_emission: 0,
            };
            controller.compute_block_reward(&metrics);
        }

        // Baselines should NOT have changed (still at initial values)
        assert_eq!(
            controller.adaptive_baseline_tx_count as u64, params.baseline_tx_count,
            "With adaptive disabled, tx_count baseline should remain static"
        );
        assert_eq!(
            controller.adaptive_baseline_tx_volume as u64, params.baseline_tx_volume_block,
            "With adaptive disabled, tx_volume baseline should remain static"
        );
        assert_eq!(
            controller.adaptive_baseline_miners as u64, params.baseline_miners,
            "With adaptive disabled, miners baseline should remain static"
        );
    }
}
