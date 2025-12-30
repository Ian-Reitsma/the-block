//! Advanced Lane-Based Dynamic Pricing Engine
//!
//! This module implements the core pricing mechanism for dual-lane transaction routing,
//! combining market signal aggregation, congestion pricing, and game-theoretic incentive
//! design to achieve economically efficient resource allocation.
//!
//! # Economic Design Principles
//!
//! 1. **Incentive Compatibility**: Users maximize utility by truthfully reporting urgency
//!    and choosing the economically appropriate lane.
//!
//! 2. **Efficiency**: Block space is allocated to highest-value uses through price discovery.
//!
//! 3. **Strategyproofness**: Gaming the system (e.g., splitting transactions, lane hopping)
//!    is economically irrational.
//!
//! 4. **Predictability**: Industrial users get guaranteed SLAs with predictable costs.
//!
//! 5. **Stability**: System resists manipulation and oscillation through smoothing and bounds.
//!
//! # Mathematical Model
//!
//! ## Consumer Lane Pricing
//!
//! The consumer lane serves P2P transactions with:
//! - Lower base fees
//! - Best-effort confirmation (no SLA)
//! - Congestion-responsive pricing
//!
//! Consumer fee formula:
//! F_c = B_c · C_c(ρ_c) · A_c(t)
//!
//! Where:
//! - B_c: base consumer fee (governance parameter)
//! - C_c(ρ_c): congestion multiplier from queueing model
//! - A_c(t): adaptive adjustment factor
//! - ρ_c: consumer lane utilization
//!
//! ## Industrial Lane Pricing
//!
//! The industrial lane serves market operations with:
//! - Higher base fees (premium for priority)
//! - Guaranteed fast confirmation (SLA: 1-2 blocks)
//! - Market demand-responsive pricing
//!
//! Industrial fee formula:
//! F_i = max(B_i · C_i(ρ_i) · M_i(D), F_c · (1 + δ))
//!
//! Where:
//! - B_i: base industrial fee
//! - C_i(ρ_i): industrial congestion multiplier
//! - M_i(D): market demand multiplier
//! - D: aggregated market demand signal ∈ [0, 1]
//! - δ: minimum industrial premium over consumer
//!
//! ## Market Demand Multiplier
//!
//! The market demand multiplier uses a logistic-inspired function:
//! M_i(D) = 1 + α · (e^(β·D) - 1) / (e^β - 1)
//!
//! Properties:
//! - M_i(0) = 1 (no market demand → no multiplier)
//! - M_i(1) = 1 + α (full demand → maximum multiplier)
//! - Smooth, monotonic, bounded
//! - α: maximum market multiplier (governance parameter)
//! - β: demand sensitivity (controls curvature)
//!
//! ## Adaptive Adjustment
//!
//! Long-term fee adjustment using proportional-integral (PI) control:
//! A(t+1) = A(t) · (1 + K_p·e(t) + K_i·∫e(t)dt)
//!
//! Where:
//! - e(t) = ρ_target - ρ_actual (utilization error)
//! - K_p: proportional gain
//! - K_i: integral gain
//!
//! This stabilizes long-term utilization around target levels.

use super::congestion::{DualLaneCongestion, CongestionReport};
use super::market_signals::{Market, MarketSignalAggregator};

/// Comprehensive lane pricing engine.
pub struct LanePricingEngine {
    /// Base fee for consumer lane (microunits per byte)
    base_consumer_fee: u64,
    /// Base fee for industrial lane (microunits per byte)
    base_industrial_fee: u64,
    /// Congestion tracking for both lanes
    congestion: DualLaneCongestion,
    /// Market signal aggregator for industrial pricing
    market_signals: MarketSignalAggregator,
    /// Adaptive adjustment factor for consumer lane
    consumer_adjustment: f64,
    /// Adaptive adjustment factor for industrial lane
    industrial_adjustment: f64,
    /// PI controller for consumer lane
    consumer_pi: PIController,
    /// PI controller for industrial lane
    industrial_pi: PIController,
    /// Market demand multiplier parameters
    market_params: MarketMultiplierParams,
    /// Target utilization for PI control
    target_utilization: f64,
}

/// Parameters for market demand multiplier M_i(D).
#[derive(Clone, Debug)]
struct MarketMultiplierParams {
    /// Maximum multiplier α (when D = 1)
    max_multiplier: f64,
    /// Sensitivity parameter β (controls curvature)
    sensitivity: f64,
}

impl MarketMultiplierParams {
    /// Compute market demand multiplier M_i(D).
    ///
    /// Formula: M_i(D) = 1 + α · (e^(β·D) - 1) / (e^β - 1)
    fn compute(&self, demand: f64) -> f64 {
        let d = demand.clamp(0.0, 1.0);

        if d < 1e-6 {
            return 1.0;
        }

        // Compute normalized exponential
        let exp_bd = (self.sensitivity * d).exp();
        let exp_b = self.sensitivity.exp();
        let normalized = (exp_bd - 1.0) / (exp_b - 1.0);

        // Apply max multiplier
        1.0 + self.max_multiplier * normalized
    }
}

/// Proportional-Integral controller for long-term fee stability.
///
/// Adjusts fees to maintain target utilization using control theory:
/// u(t) = K_p · e(t) + K_i · Σe(t)
///
/// Where:
/// - e(t) = target - actual (error)
/// - K_p: proportional gain (responds to current error)
/// - K_i: integral gain (responds to accumulated error)
#[derive(Clone, Debug)]
struct PIController {
    /// Proportional gain K_p
    kp: f64,
    /// Integral gain K_i
    ki: f64,
    /// Accumulated error ∫e(t)dt
    integral: f64,
    /// Anti-windup limit for integral term
    integral_limit: f64,
}

impl PIController {
    fn new(kp: f64, ki: f64, integral_limit: f64) -> Self {
        Self {
            kp,
            ki,
            integral: 0.0,
            integral_limit,
        }
    }

    /// Update controller with new utilization measurement.
    ///
    /// Returns adjustment factor (multiplicative) ∈ [0.5, 2.0].
    fn update(&mut self, target: f64, actual: f64) -> f64 {
        let error = target - actual;

        // Update integral with anti-windup
        self.integral += error;
        self.integral = self.integral.clamp(-self.integral_limit, self.integral_limit);

        // Compute control signal
        let control = self.kp * error + self.ki * self.integral;

        // Convert to multiplicative adjustment factor
        // control = 0 → factor = 1.0 (no change)
        // control > 0 → factor > 1.0 (increase fees)
        // control < 0 → factor < 1.0 (decrease fees)
        let factor = 1.0 + control;

        // Bound adjustment to prevent instability
        factor.clamp(0.5, 2.0)
    }

    fn reset(&mut self) {
        self.integral = 0.0;
    }
}

impl LanePricingEngine {
    /// Create new lane pricing engine with specified parameters.
    ///
    /// # Arguments
    /// * `base_consumer_fee` - Base consumer lane fee (microunits/byte)
    /// * `base_industrial_fee` - Base industrial lane fee (microunits/byte)
    /// * `consumer_capacity` - Max consumer transactions per block
    /// * `industrial_capacity` - Max industrial transactions per block
    /// * `target_utilization` - Target utilization for PI control (e.g., 0.7 = 70%)
    pub fn new(
        base_consumer_fee: u64,
        base_industrial_fee: u64,
        consumer_capacity: f64,
        industrial_capacity: f64,
        target_utilization: f64,
    ) -> Self {
        // Congestion parameters
        let window_size = 50; // 50 blocks ~10 minutes
        let consumer_sensitivity = 3.0; // Moderate congestion response for P2P
        let industrial_sensitivity = 5.0; // Aggressive congestion response for priority
        let min_industrial_premium = 0.5; // Industrial ≥ 150% of consumer

        let congestion = DualLaneCongestion::new(
            consumer_capacity,
            industrial_capacity,
            window_size,
            consumer_sensitivity,
            industrial_sensitivity,
            min_industrial_premium,
        );

        // Market signal parameters
        let signal_half_life = 50.0; // 50 blocks smoothing
        let signal_weights = (0.4, 0.3, 0.3); // (price, volume, utilization)
        let market_signals = MarketSignalAggregator::new(signal_half_life, signal_weights);

        // Market multiplier parameters
        let market_params = MarketMultiplierParams {
            max_multiplier: 3.0, // Max 4x multiplier at full demand
            sensitivity: 2.0,    // Moderate exponential curvature
        };

        // PI controller parameters (tuned for stability)
        let kp = 0.1; // Proportional gain
        let ki = 0.01; // Integral gain
        let integral_limit = 5.0; // Anti-windup limit

        Self {
            base_consumer_fee,
            base_industrial_fee,
            congestion,
            market_signals,
            consumer_adjustment: 1.0,
            industrial_adjustment: 1.0,
            consumer_pi: PIController::new(kp, ki, integral_limit),
            industrial_pi: PIController::new(kp, ki, integral_limit),
            market_params,
            target_utilization: target_utilization.clamp(0.3, 0.9),
        }
    }

    /// Update pricing engine with new block data.
    ///
    /// Called after each block to update congestion metrics and adaptive adjustments.
    pub fn update_block(&mut self, consumer_tx_count: u64, industrial_tx_count: u64) {
        // Update congestion tracking
        self.congestion.update_both(consumer_tx_count, industrial_tx_count);

        // Update adaptive adjustments using PI control
        let consumer_util = self.congestion.consumer.utilization();
        let industrial_util = self.congestion.industrial.utilization();

        self.consumer_adjustment = self.consumer_pi.update(self.target_utilization, consumer_util);
        self.industrial_adjustment = self.industrial_pi.update(self.target_utilization, industrial_util);
    }

    /// Update market signal for industrial lane pricing.
    ///
    /// Called when market events occur (ad settlement, energy oracle, compute job).
    pub fn update_market_signal(
        &mut self,
        market: Market,
        clearing_price: u64,
        volume: u64,
        utilization: f64,
    ) {
        self.market_signals.update_market(market, clearing_price, volume, utilization);
    }

    /// Compute current consumer lane fee per byte.
    ///
    /// Formula: F_c = B_c · C_c(ρ_c) · A_c(t)
    pub fn consumer_fee_per_byte(&self) -> u64 {
        let base = self.base_consumer_fee as f64;
        let congestion = self.congestion.consumer.multiplier();
        let adjustment = self.consumer_adjustment;

        let fee = base * congestion * adjustment;
        // Only enforce minimum of 1 if base fee is non-zero (allows zero fees for testing)
        if self.base_consumer_fee == 0 {
            0
        } else {
            fee.ceil().max(1.0) as u64
        }
    }

    /// Compute current industrial lane fee per byte.
    ///
    /// Formula: F_i = max(B_i · C_i(ρ_i) · M_i(D) · A_i(t), F_c · (1 + δ))
    pub fn industrial_fee_per_byte(&self) -> u64 {
        // Allow zero fees for testing when base is zero
        if self.base_industrial_fee == 0 {
            return 0;
        }

        let base = self.base_industrial_fee as f64;
        let congestion = self.congestion.industrial.multiplier();
        let market_demand = self.market_signals.aggregate_demand();
        let market_multiplier = self.market_params.compute(market_demand);
        let adjustment = self.industrial_adjustment;

        let base_industrial = base * congestion * market_multiplier * adjustment;

        // Enforce minimum premium over consumer lane
        let consumer_fee = self.consumer_fee_per_byte();
        let min_industrial = if consumer_fee > 0 {
            ((consumer_fee as f64) * 1.5).ceil() // 50% premium
        } else {
            1.0 // If consumer is zero, enforce minimum of 1 for industrial
        };

        base_industrial.ceil().max(min_industrial) as u64
    }

    /// Get comprehensive pricing report for monitoring.
    pub fn pricing_report(&self) -> PricingReport {
        PricingReport {
            consumer_fee_per_byte: self.consumer_fee_per_byte(),
            industrial_fee_per_byte: self.industrial_fee_per_byte(),
            consumer_adjustment: self.consumer_adjustment,
            industrial_adjustment: self.industrial_adjustment,
            market_demand: self.market_signals.aggregate_demand(),
            congestion: self.congestion.report(),
        }
    }

    /// Estimate fee for a transaction given size and lane.
    pub fn estimate_fee(&self, size_bytes: u64, is_industrial: bool) -> u64 {
        let fee_per_byte = if is_industrial {
            self.industrial_fee_per_byte()
        } else {
            self.consumer_fee_per_byte()
        };

        fee_per_byte.saturating_mul(size_bytes)
    }

    /// Check if a transaction would be admitted to consumer lane.
    ///
    /// Rejects if lane would overflow or if transaction type requires industrial.
    pub fn would_admit_consumer(&self, tx_count: u64) -> bool {
        !self.congestion.consumer_would_overflow(tx_count)
    }

    /// Check if a transaction would be admitted to industrial lane.
    pub fn would_admit_industrial(&self, tx_count: u64) -> bool {
        !self.congestion.industrial_would_overflow(tx_count)
    }

    /// Update base fees (governance-controlled).
    pub fn set_base_fees(&mut self, consumer: u64, industrial: u64) {
        self.base_consumer_fee = consumer.max(1);
        self.base_industrial_fee = industrial.max(1);
    }

    /// Update target utilization for PI control.
    pub fn set_target_utilization(&mut self, target: f64) {
        self.target_utilization = target.clamp(0.3, 0.9);
    }

    /// Reset adaptive state (PI controller integrals).
    ///
    /// Call this when governance parameters change significantly to prevent
    /// accumulated error from causing incorrect adjustments.
    pub fn reset_adaptive_state(&mut self) {
        self.consumer_pi.reset();
        self.industrial_pi.reset();
        self.consumer_adjustment = 1.0;
        self.industrial_adjustment = 1.0;
    }
}

/// Comprehensive pricing report for telemetry and monitoring.
#[derive(Clone, Debug)]
pub struct PricingReport {
    pub consumer_fee_per_byte: u64,
    pub industrial_fee_per_byte: u64,
    pub consumer_adjustment: f64,
    pub industrial_adjustment: f64,
    pub market_demand: f64,
    pub congestion: CongestionReport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_multiplier_bounds() {
        let params = MarketMultiplierParams {
            max_multiplier: 3.0,
            sensitivity: 2.0,
        };

        // No demand → multiplier = 1
        assert!((params.compute(0.0) - 1.0).abs() < 0.01);

        // Full demand → multiplier = 1 + max
        let m_full = params.compute(1.0);
        assert!((m_full - 4.0).abs() < 0.01);

        // Monotonic increase
        let m_half = params.compute(0.5);
        assert!(m_half > 1.0 && m_half < m_full);
    }

    #[test]
    fn pi_controller_stability() {
        let mut pi = PIController::new(0.1, 0.01, 5.0);

        // System below target → increase fees
        let adj1 = pi.update(0.7, 0.5);
        assert!(adj1 > 1.0);

        // System above target → decrease fees
        let adj2 = pi.update(0.7, 0.9);
        assert!(adj2 < 1.0);

        // Adjustments should be bounded
        assert!(adj1 >= 0.5 && adj1 <= 2.0);
        assert!(adj2 >= 0.5 && adj2 <= 2.0);
    }

    #[test]
    fn industrial_premium_enforced() {
        let mut engine = LanePricingEngine::new(
            1000, // consumer base
            1500, // industrial base
            100.0, 100.0, // capacities
            0.7, // target util
        );

        // Even with equal congestion and zero market demand
        engine.update_block(0, 0);

        let consumer = engine.consumer_fee_per_byte();
        let industrial = engine.industrial_fee_per_byte();

        // Industrial must be ≥ 150% of consumer
        assert!(industrial >= ((consumer as f64) * 1.5) as u64);
    }

    #[test]
    fn market_demand_increases_industrial_fee() {
        let mut engine = LanePricingEngine::new(1000, 1500, 100.0, 100.0, 0.7);

        // Baseline with no market demand
        engine.update_block(0, 0);
        let baseline = engine.industrial_fee_per_byte();

        // Add significant market demand
        engine.update_market_signal(Market::Advertising, 100_000, 50, 0.9);
        engine.update_market_signal(Market::Energy, 50_000, 30, 0.8);
        engine.update_market_signal(Market::Compute, 75_000, 40, 0.85);

        let with_demand = engine.industrial_fee_per_byte();

        // Industrial fee should increase with market demand
        assert!(with_demand > baseline);
    }

    #[test]
    fn congestion_increases_fees() {
        let mut engine = LanePricingEngine::new(1000, 1500, 100.0, 100.0, 0.7);

        // Low congestion
        engine.update_block(10, 10);
        let low_consumer = engine.consumer_fee_per_byte();
        let low_industrial = engine.industrial_fee_per_byte();

        // High congestion
        for _ in 0..20 {
            engine.update_block(90, 90);
        }
        let high_consumer = engine.consumer_fee_per_byte();
        let high_industrial = engine.industrial_fee_per_byte();

        // Fees should increase with congestion
        assert!(high_consumer > low_consumer);
        assert!(high_industrial > low_industrial);
    }
}
