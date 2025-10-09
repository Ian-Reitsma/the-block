#![forbid(unsafe_code)]

use foundation_serialization::{Deserialize, Serialize};

/// Simple inflation model tracking total token supply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflationModel {
    /// Annualised inflation rate expressed as a fraction.
    pub rate: f64,
    /// Current total supply.
    pub supply: f64,
}

impl Default for InflationModel {
    fn default() -> Self {
        Self {
            rate: 0.01,
            supply: 0.0,
        }
    }
}

impl InflationModel {
    /// Apply inflation to the provided base amount and update total supply.
    pub fn apply(&mut self, base: f64) -> f64 {
        self.supply += base * self.rate;
        self.supply
    }
}
