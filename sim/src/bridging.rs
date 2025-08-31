#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Models cross-chain bridging flows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeModel {
    /// Total amount bridged out of the system.
    pub bridged: f64,
}

impl Default for BridgeModel {
    fn default() -> Self {
        Self { bridged: 0.0 }
    }
}

impl BridgeModel {
    /// Apply a bridging flow proportional to `amount`.
    pub fn flow(&mut self, amount: f64) -> f64 {
        // Assume half of the amount is bridged to external chains.
        self.bridged += amount * 0.5;
        self.bridged
    }
}
