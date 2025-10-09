#![forbid(unsafe_code)]

use foundation_serialization::{Deserialize, Serialize};

/// Models demand growth for consumer and industrial usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandModel {
    pub consumer_growth: f64,
    pub industrial_growth: f64,
}

impl Default for DemandModel {
    fn default() -> Self {
        Self {
            consumer_growth: 0.02,
            industrial_growth: 0.03,
        }
    }
}

impl DemandModel {
    /// Advance demand projections one step.
    pub fn project(&mut self) -> (f64, f64) {
        self.consumer_growth *= 1.01;
        self.industrial_growth *= 1.01;
        (self.consumer_growth, self.industrial_growth)
    }
}
