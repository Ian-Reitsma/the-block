//! Lane-Specific Congestion Tracking and Pricing
//!
//! This module implements sophisticated congestion metrics for consumer and industrial
//! lanes, using queueing theory and congestion pricing models from transportation
//! economics.
//!
//! # Economic Theory
//!
//! Congestion pricing internalizes the externality cost that each transaction imposes
//! on others by consuming scarce block space. The optimal congestion charge equals
//! the marginal external cost at equilibrium.
//!
//! # Mathematical Model
//!
//! We model each lane as an M/M/1 queue with:
//! - λ: arrival rate (transactions per block)
//! - μ: service rate (max transactions per block)
//! - ρ = λ/μ: utilization ratio
//!
//! Queue length: L = ρ/(1-ρ) for ρ < 1
//! Wait time: W = L/λ = 1/(μ-λ)
//!
//! Congestion multiplier uses a superlinear penalty function:
//! C(ρ) = 1 + k·(ρ/(1-ρ))^n
//!
//! Where:
//! - k: congestion sensitivity parameter
//! - n: superlinearity exponent (typically 2-3)
//! - As ρ → 1, C(ρ) → ∞ (prevents overload)
//!
//! # Cross-Lane Arbitrage Prevention
//!
//! To prevent users from gaming the system by splitting transactions across lanes,
//! we enforce:
//! 1. Minimum fee differential: industrial_fee ≥ consumer_fee * (1 + δ)
//! 2. SLA-based pricing: industrial lane guarantees faster confirmation
//! 3. Type-based admission: market ops rejected from consumer lane

use std::collections::VecDeque;

/// Lane-specific congestion state using queueing theory.
#[derive(Clone, Debug)]
pub struct LaneCongestion {
    /// Lane identifier
    lane: &'static str,
    /// Maximum transactions per block (service rate μ)
    max_tx_per_block: f64,
    /// Rolling window of arrival rates (λ) per block
    arrival_window: VecDeque<f64>,
    /// Window size for averaging
    window_size: usize,
    /// Current utilization ratio ρ = λ/μ
    utilization: f64,
    /// Current congestion multiplier C(ρ)
    congestion_multiplier: f64,
    /// Congestion sensitivity parameter k
    sensitivity: f64,
    /// Superlinearity exponent n
    exponent: f64,
    /// Minimum multiplier (when empty)
    min_multiplier: f64,
    /// Maximum multiplier (safety cap)
    max_multiplier: f64,
}

impl LaneCongestion {
    /// Create new congestion tracker for a lane.
    ///
    /// # Arguments
    /// * `lane` - Lane name ("consumer" or "industrial")
    /// * `max_tx_per_block` - Service capacity μ
    /// * `window_size` - Blocks to average for arrival rate
    /// * `sensitivity` - Congestion sensitivity k (higher = more aggressive pricing)
    /// * `exponent` - Superlinearity n (typically 2.0-3.0)
    pub fn new(
        lane: &'static str,
        max_tx_per_block: f64,
        window_size: usize,
        sensitivity: f64,
        exponent: f64,
    ) -> Self {
        Self {
            lane,
            max_tx_per_block: max_tx_per_block.max(1.0),
            arrival_window: VecDeque::with_capacity(window_size),
            window_size: window_size.max(1),
            utilization: 0.0,
            congestion_multiplier: 1.0,
            sensitivity: sensitivity.max(0.0),
            exponent: exponent.max(1.0),
            min_multiplier: 1.0,
            max_multiplier: 1000.0, // Prevent infinite fees
        }
    }

    /// Update congestion state with new block data.
    ///
    /// # Arguments
    /// * `tx_count` - Number of transactions processed this block
    ///
    /// # Returns
    /// New congestion multiplier C(ρ)
    pub fn update(&mut self, tx_count: u64) -> f64 {
        let arrival_rate = tx_count as f64;

        // Update rolling window
        if self.arrival_window.len() >= self.window_size {
            self.arrival_window.pop_front();
        }
        self.arrival_window.push_back(arrival_rate);

        // Compute average arrival rate λ
        let lambda = if self.arrival_window.is_empty() {
            0.0
        } else {
            self.arrival_window.iter().sum::<f64>() / self.arrival_window.len() as f64
        };

        // Compute utilization ρ = λ/μ
        self.utilization = lambda / self.max_tx_per_block;

        // Compute congestion multiplier C(ρ)
        self.congestion_multiplier = self.compute_multiplier(self.utilization);
        self.congestion_multiplier
    }

    /// Compute congestion multiplier from utilization ratio.
    ///
    /// Formula: C(ρ) = 1 + k·(ρ/(1-ρ))^n
    ///
    /// This is a superlinear penalty that:
    /// - Equals 1.0 when empty (no congestion)
    /// - Increases slowly at low utilization
    /// - Increases rapidly as ρ → 1
    /// - Approaches infinity as ρ → 1 (prevents overload)
    fn compute_multiplier(&self, rho: f64) -> f64 {
        // Clamp utilization to prevent numerical issues
        let rho_clamped = rho.clamp(0.0, 0.999);

        if rho_clamped < 1e-6 {
            return self.min_multiplier;
        }

        // Queue length formula: ρ/(1-ρ)
        let queue_factor = rho_clamped / (1.0 - rho_clamped);

        // Superlinear penalty: (ρ/(1-ρ))^n
        let penalty = queue_factor.powf(self.exponent);

        // Final multiplier: 1 + k·penalty
        let multiplier = self.min_multiplier + self.sensitivity * penalty;

        // Apply safety bounds
        multiplier.clamp(self.min_multiplier, self.max_multiplier)
    }

    /// Get lane identifier.
    pub fn lane(&self) -> &'static str {
        self.lane
    }

    /// Get current utilization ratio ρ ∈ [0, 1].
    pub fn utilization(&self) -> f64 {
        self.utilization
    }

    /// Get current congestion multiplier C(ρ) ≥ 1.
    pub fn multiplier(&self) -> f64 {
        self.congestion_multiplier
    }

    /// Get current arrival rate λ (average transactions per block).
    pub fn arrival_rate(&self) -> f64 {
        if self.arrival_window.is_empty() {
            0.0
        } else {
            self.arrival_window.iter().sum::<f64>() / self.arrival_window.len() as f64
        }
    }

    /// Estimate expected wait time in blocks using W = 1/(μ-λ).
    ///
    /// Returns None if queue is unstable (λ ≥ μ).
    pub fn expected_wait_blocks(&self) -> Option<f64> {
        let lambda = self.arrival_rate();
        if lambda >= self.max_tx_per_block {
            None // Queue unstable
        } else {
            Some(1.0 / (self.max_tx_per_block - lambda))
        }
    }

    /// Predict congestion multiplier if N additional transactions arrive.
    ///
    /// Useful for showing users the marginal impact of their transaction.
    pub fn predict_multiplier(&self, additional_tx: u64) -> f64 {
        let current_lambda = self.arrival_rate();
        let new_lambda = current_lambda + additional_tx as f64;
        let new_rho = new_lambda / self.max_tx_per_block;
        self.compute_multiplier(new_rho)
    }
}

/// Dual-lane congestion manager coordinating consumer and industrial lanes.
///
/// Enforces economic properties:
/// 1. Industrial lane always has higher base fee (priority guarantee)
/// 2. Congestion pricing is lane-specific
/// 3. Cross-lane arbitrage is prevented via minimum differential
pub struct DualLaneCongestion {
    /// Consumer lane (P2P transactions, slow, lower fees)
    pub consumer: LaneCongestion,
    /// Industrial lane (market operations, fast, higher fees)
    pub industrial: LaneCongestion,
    /// Minimum industrial/consumer fee ratio δ
    /// Ensures industrial_fee ≥ consumer_fee * (1 + delta)
    min_industrial_premium: f64,
}

impl DualLaneCongestion {
    /// Create new dual-lane congestion manager.
    ///
    /// # Arguments
    /// * `consumer_capacity` - Max consumer tx per block
    /// * `industrial_capacity` - Max industrial tx per block
    /// * `window_size` - Blocks for arrival rate averaging
    /// * `consumer_sensitivity` - Consumer lane congestion sensitivity
    /// * `industrial_sensitivity` - Industrial lane congestion sensitivity
    /// * `min_industrial_premium` - Minimum industrial/consumer fee ratio (e.g., 0.5 = 50% premium)
    pub fn new(
        consumer_capacity: f64,
        industrial_capacity: f64,
        window_size: usize,
        consumer_sensitivity: f64,
        industrial_sensitivity: f64,
        min_industrial_premium: f64,
    ) -> Self {
        Self {
            consumer: LaneCongestion::new(
                "consumer",
                consumer_capacity,
                window_size,
                consumer_sensitivity,
                2.0, // Moderate superlinearity for P2P
            ),
            industrial: LaneCongestion::new(
                "industrial",
                industrial_capacity,
                window_size,
                industrial_sensitivity,
                2.5, // Higher superlinearity for priority lane
            ),
            min_industrial_premium: min_industrial_premium.max(0.0),
        }
    }

    /// Update both lanes with new block data.
    pub fn update_both(&mut self, consumer_tx: u64, industrial_tx: u64) {
        self.consumer.update(consumer_tx);
        self.industrial.update(industrial_tx);
    }

    /// Compute final fee for consumer lane given base fee.
    ///
    /// final_fee = base_fee * congestion_multiplier
    pub fn consumer_fee(&self, base_fee: u64) -> u64 {
        let multiplier = self.consumer.multiplier();
        ((base_fee as f64) * multiplier).ceil() as u64
    }

    /// Compute final fee for industrial lane given base fee.
    ///
    /// Enforces minimum premium over consumer lane:
    /// industrial_fee ≥ max(base_industrial, consumer_fee * (1 + δ))
    pub fn industrial_fee(&self, base_fee: u64) -> u64 {
        let multiplier = self.industrial.multiplier();
        let base_industrial = ((base_fee as f64) * multiplier).ceil() as u64;

        // Compute minimum based on consumer lane
        let consumer_fee = self.consumer_fee(base_fee);
        let min_industrial =
            ((consumer_fee as f64) * (1.0 + self.min_industrial_premium)).ceil() as u64;

        base_industrial.max(min_industrial)
    }

    /// Check if adding a transaction to consumer lane would cause instability.
    pub fn consumer_would_overflow(&self, additional_tx: u64) -> bool {
        let lambda = self.consumer.arrival_rate() + additional_tx as f64;
        lambda >= self.consumer.max_tx_per_block
    }

    /// Check if adding a transaction to industrial lane would cause instability.
    pub fn industrial_would_overflow(&self, additional_tx: u64) -> bool {
        let lambda = self.industrial.arrival_rate() + additional_tx as f64;
        lambda >= self.industrial.max_tx_per_block
    }

    /// Get comprehensive congestion report for both lanes.
    pub fn report(&self) -> CongestionReport {
        CongestionReport {
            consumer_utilization: self.consumer.utilization(),
            consumer_multiplier: self.consumer.multiplier(),
            consumer_wait_blocks: self.consumer.expected_wait_blocks(),
            industrial_utilization: self.industrial.utilization(),
            industrial_multiplier: self.industrial.multiplier(),
            industrial_wait_blocks: self.industrial.expected_wait_blocks(),
        }
    }
}

/// Congestion metrics report for monitoring and telemetry.
#[derive(Clone, Debug)]
pub struct CongestionReport {
    pub consumer_utilization: f64,
    pub consumer_multiplier: f64,
    pub consumer_wait_blocks: Option<f64>,
    pub industrial_utilization: f64,
    pub industrial_multiplier: f64,
    pub industrial_wait_blocks: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn congestion_multiplier_increases_with_load() {
        let mut lane = LaneCongestion::new("test", 100.0, 10, 5.0, 2.0);

        // Low load: multiplier near 1
        let m1 = lane.update(10);
        assert!(m1 < 1.5);

        // Medium load: multiplier increases
        for _ in 0..10 {
            lane.update(50);
        }
        let m2 = lane.multiplier();
        assert!(m2 > m1);
        assert!(m2 < 10.0);

        // High load: multiplier increases significantly
        for _ in 0..10 {
            lane.update(95);
        }
        let m3 = lane.multiplier();
        assert!(m3 > m2);
        assert!(m3 > 10.0);
    }

    #[test]
    fn dual_lane_enforces_industrial_premium() {
        let mut dual = DualLaneCongestion::new(
            100.0, // consumer capacity
            100.0, // industrial capacity
            10,    // window
            5.0,   // consumer sensitivity
            5.0,   // industrial sensitivity
            0.5,   // 50% minimum premium
        );

        // Equal congestion
        dual.update_both(50, 50);

        let consumer = dual.consumer_fee(1000);
        let industrial = dual.industrial_fee(1000);

        // Industrial must be at least 50% higher
        assert!(industrial >= ((consumer as f64) * 1.5) as u64);
    }

    #[test]
    fn overflow_detection() {
        let mut dual = DualLaneCongestion::new(100.0, 100.0, 10, 5.0, 5.0, 0.5);

        // Fill up consumer lane
        for _ in 0..10 {
            dual.consumer.update(95);
        }

        // Should detect potential overflow
        assert!(dual.consumer_would_overflow(10));
        assert!(!dual.consumer_would_overflow(1));
    }

    #[test]
    fn wait_time_estimation() {
        let mut lane = LaneCongestion::new("test", 100.0, 10, 5.0, 2.0);

        // Low load: short wait
        lane.update(10);
        let wait1 = lane.expected_wait_blocks().unwrap();
        assert!(wait1 < 1.5);

        // Higher load: longer wait
        for _ in 0..10 {
            lane.update(80);
        }
        let wait2 = lane.expected_wait_blocks().unwrap();
        assert!(wait2 > wait1);
    }
}
