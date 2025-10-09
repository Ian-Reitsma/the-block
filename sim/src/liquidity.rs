#![forbid(unsafe_code)]

use foundation_serialization::{Deserialize, Serialize};

/// Tracks token liquidity available for simulations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityModel {
    /// Token reserves across simulated markets.
    pub token_reserve: f64,
}

impl Default for LiquidityModel {
    fn default() -> Self {
        Self { token_reserve: 0.0 }
    }
}

impl LiquidityModel {
    /// Update reserves with an inflow amount.
    pub fn update(&mut self, inflow: f64) -> f64 {
        self.token_reserve += inflow;
        self.token_reserve
    }
}
