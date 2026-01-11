//! Market Signal Aggregation for Industrial Lane Pricing
//!
//! This module aggregates real-time demand signals from all markets (ad, energy, compute)
//! to inform dynamic pricing for the industrial transaction lane. The industrial lane
//! serves market operations requiring fast confirmation with predictable costs.
//!
//! # Economic Theory
//!
//! Uses a weighted exponential moving average (EMA) to smooth market signals while
//! maintaining responsiveness to genuine demand shifts. The aggregation function
//! combines heterogeneous market signals into a unified demand metric using
//! variance-normalized weights to prevent any single market from dominating.
//!
//! # Mathematical Model
//!
//! Let M = {ad, energy, compute} be the set of markets.
//! For each market m ∈ M, we track:
//! - p_m(t): market clearing price at time t
//! - v_m(t): transaction volume at time t
//! - u_m(t): utilization ratio at time t ∈ [0, 1]
//!
//! The market demand signal is:
//! s_m(t) = α·p_m(t) + β·v_m(t) + γ·u_m(t)
//!
//! Where α, β, γ are calibrated weights satisfying α + β + γ = 1.
//!
//! The aggregated industrial demand is:
//! D_industrial(t) = Σ_m w_m · EMA(s_m(t), λ)
//!
//! Where w_m are variance-normalized market weights and λ is the EMA smoothing factor.

use std::collections::HashMap;

/// Exponential moving average smoother with configurable half-life.
///
/// EMA formula: EMA(t) = α·x(t) + (1-α)·EMA(t-1)
/// where α = 1 - exp(-ln(2)/half_life)
#[derive(Clone, Debug)]
pub struct ExponentialSmoother {
    /// Current EMA value
    value: f64,
    /// Smoothing coefficient α ∈ (0, 1)
    alpha: f64,
    /// Number of observations
    count: u64,
}

impl ExponentialSmoother {
    /// Create new smoother with specified half-life in blocks.
    ///
    /// Half-life is the number of blocks for the EMA weight to decay by 50%.
    /// Smaller half-life = more responsive to recent changes.
    /// Larger half-life = smoother, less reactive.
    pub fn new(half_life_blocks: f64) -> Self {
        let alpha = 1.0 - (-std::f64::consts::LN_2 / half_life_blocks.max(1.0)).exp();
        Self {
            value: 0.0,
            alpha,
            count: 0,
        }
    }

    /// Update with new observation and return current EMA.
    pub fn update(&mut self, observation: f64) -> f64 {
        if self.count == 0 {
            self.value = observation;
        } else {
            self.value = self.alpha * observation + (1.0 - self.alpha) * self.value;
        }
        self.count += 1;
        self.value
    }

    /// Get current EMA value without updating.
    pub fn current(&self) -> f64 {
        self.value
    }

    /// Reset smoother to initial state.
    pub fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
    }
}

/// Market-specific demand metrics.
#[derive(Clone, Debug)]
pub struct MarketMetrics {
    /// Clearing price in microunits (e.g., USD micros for ad market)
    pub clearing_price: ExponentialSmoother,
    /// Transaction volume (number of transactions)
    pub volume: ExponentialSmoother,
    /// Utilization ratio ∈ [0, 1]
    pub utilization: ExponentialSmoother,
    /// Composite demand signal
    pub demand_signal: f64,
}

impl MarketMetrics {
    pub fn new(half_life_blocks: f64) -> Self {
        Self {
            clearing_price: ExponentialSmoother::new(half_life_blocks),
            volume: ExponentialSmoother::new(half_life_blocks),
            utilization: ExponentialSmoother::new(half_life_blocks),
            demand_signal: 0.0,
        }
    }

    /// Update all metrics and compute composite demand signal.
    ///
    /// # Arguments
    /// * `price` - Current clearing price (microunits)
    /// * `volume` - Number of transactions this block
    /// * `utilization` - Utilization ratio [0, 1]
    /// * `weights` - (α, β, γ) weights for price, volume, utilization
    ///
    /// # Returns
    /// Updated demand signal s_m(t)
    pub fn update(
        &mut self,
        price: u64,
        volume: u64,
        utilization: f64,
        weights: (f64, f64, f64),
    ) -> f64 {
        let (alpha, beta, gamma) = weights;

        // Update EMAs
        let p = self.clearing_price.update(price as f64);
        let v = self.volume.update(volume as f64);
        let u = self.utilization.update(utilization.clamp(0.0, 1.0));

        // Normalize each component to [0, 1] range using adaptive scaling
        // This prevents any single component from dominating the signal
        let p_norm = (p / (p + 1000.0)).clamp(0.0, 1.0); // Asymptotic normalization
        let v_norm = (v / (v + 10.0)).clamp(0.0, 1.0);
        let u_norm = u;

        // Compute weighted composite signal
        self.demand_signal = alpha * p_norm + beta * v_norm + gamma * u_norm;
        self.demand_signal
    }

    pub fn current_signal(&self) -> f64 {
        self.demand_signal
    }
}

/// Market identifiers for signal aggregation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Market {
    /// Advertising market (ad auctions, impressions)
    Advertising,
    /// Energy oracle market (proof-of-stake, validation)
    Energy,
    /// Compute market (job execution, settlements)
    Compute,
}

/// Aggregates demand signals across all markets to inform industrial lane pricing.
///
/// # Variance-Normalized Weighting
///
/// Markets with high variance contribute less to the aggregate signal to prevent
/// volatility spillover. Weights are dynamically adjusted based on rolling variance:
///
/// w_m = (1/σ²_m) / Σ_k(1/σ²_k)
///
/// Where σ²_m is the variance of market m's demand signal.
pub struct MarketSignalAggregator {
    /// Per-market metrics
    markets: HashMap<Market, MarketMetrics>,
    /// Variance trackers for dynamic weighting
    variance: HashMap<Market, RunningVariance>,
    /// Signal combination weights (α, β, γ) for (price, volume, utilization)
    signal_weights: (f64, f64, f64),
    /// Aggregated industrial demand metric
    aggregate_demand: f64,
}

impl MarketSignalAggregator {
    /// Create new aggregator with specified EMA half-life.
    ///
    /// # Arguments
    /// * `half_life_blocks` - EMA half-life for smoothing (default: 50 blocks ~10 min)
    /// * `signal_weights` - (α, β, γ) weights, must sum to 1.0
    pub fn new(half_life_blocks: f64, signal_weights: (f64, f64, f64)) -> Self {
        let mut markets = HashMap::new();
        let mut variance = HashMap::new();

        for market in [Market::Advertising, Market::Energy, Market::Compute] {
            markets.insert(market, MarketMetrics::new(half_life_blocks));
            variance.insert(market, RunningVariance::new());
        }

        // Normalize weights
        let (a, b, c) = signal_weights;
        let sum = a + b + c;
        let signal_weights = if sum > 0.0 {
            (a / sum, b / sum, c / sum)
        } else {
            (0.33, 0.33, 0.34) // Default equal weighting
        };

        Self {
            markets,
            variance,
            signal_weights,
            aggregate_demand: 0.0,
        }
    }

    /// Update a specific market's metrics and recompute aggregate demand.
    pub fn update_market(
        &mut self,
        market: Market,
        clearing_price: u64,
        volume: u64,
        utilization: f64,
    ) -> f64 {
        // Update market-specific signal
        if let Some(metrics) = self.markets.get_mut(&market) {
            let signal = metrics.update(clearing_price, volume, utilization, self.signal_weights);

            // Update variance tracker
            if let Some(var) = self.variance.get_mut(&market) {
                var.update(signal);
            }
        }

        // Recompute aggregate with variance-normalized weights
        self.recompute_aggregate()
    }

    /// Recompute aggregate demand using variance-normalized market weights.
    fn recompute_aggregate(&mut self) -> f64 {
        let mut total_inv_var = 0.0;
        let mut weighted_sum = 0.0;

        for (&market, metrics) in &self.markets {
            if let Some(var_tracker) = self.variance.get(&market) {
                let variance = var_tracker.variance().max(1e-9); // Avoid division by zero
                let inv_var = 1.0 / variance;
                total_inv_var += inv_var;
                weighted_sum += inv_var * metrics.current_signal();
            }
        }

        self.aggregate_demand = if total_inv_var > 0.0 {
            weighted_sum / total_inv_var
        } else {
            0.0
        };

        self.aggregate_demand
    }

    /// Get current aggregated industrial demand ∈ [0, 1].
    pub fn aggregate_demand(&self) -> f64 {
        self.aggregate_demand
    }

    /// Get signal for specific market.
    pub fn market_signal(&self, market: Market) -> f64 {
        self.markets
            .get(&market)
            .map(|m| m.current_signal())
            .unwrap_or(0.0)
    }
}

/// Running variance calculator using Welford's online algorithm.
///
/// Computes variance in a single pass with numerical stability:
/// M_k = M_{k-1} + (x_k - M_{k-1})/k
/// S_k = S_{k-1} + (x_k - M_{k-1})(x_k - M_k)
/// σ² = S_k / (k - 1)
#[derive(Clone, Debug)]
struct RunningVariance {
    count: u64,
    mean: f64,
    m2: f64,
}

impl RunningVariance {
    fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    fn update(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    fn variance(&self) -> f64 {
        if self.count < 2 {
            0.0
        } else {
            self.m2 / (self.count - 1) as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_smoother_converges() {
        let mut smoother = ExponentialSmoother::new(10.0);

        // Feed constant value
        for _ in 0..100 {
            smoother.update(100.0);
        }

        // Should converge to input value
        assert!((smoother.current() - 100.0).abs() < 0.1);
    }

    #[test]
    fn market_metrics_bounded() {
        let mut metrics = MarketMetrics::new(10.0);

        // Extreme inputs
        let signal = metrics.update(1_000_000, 1000, 0.99, (0.33, 0.33, 0.34));

        // Signal should be bounded [0, 1]
        assert!(signal >= 0.0 && signal <= 1.0);
    }

    #[test]
    fn variance_normalized_weighting() {
        let mut agg = MarketSignalAggregator::new(10.0, (0.4, 0.3, 0.3));

        // Stable market
        for _ in 0..10 {
            agg.update_market(Market::Advertising, 1000, 10, 0.5);
        }

        // Volatile market
        for i in 0..10 {
            let price = if i % 2 == 0 { 100 } else { 10000 };
            agg.update_market(Market::Energy, price, 5, 0.3);
        }

        // Aggregate should exist and be bounded
        let demand = agg.aggregate_demand();
        assert!(demand >= 0.0 && demand <= 1.0);
    }
}
