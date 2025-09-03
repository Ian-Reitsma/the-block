use serde::{Deserialize, Serialize};

/// Redeemable compute-backed token.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ComputeToken {
    /// Number of compute units this token represents.
    pub units: u64,
}

impl ComputeToken {
    /// Redeem tokens for compute units using the provided redeem curve and
    /// backstop. Returns the number of units granted.
    pub fn redeem(
        &self,
        curve: &RedeemCurve,
        backstop: &mut Backstop,
    ) -> Result<u64, &'static str> {
        let units = curve.redeem(self.units);
        if backstop.reserve < units {
            return Err("backstop depleted");
        }
        backstop.reserve -= units;
        Ok(units)
    }
}

/// Linear redeem curve priced off the current marketplace median.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RedeemCurve {
    /// Base price per compute unit.
    pub base: u64,
    /// Slope adjustment in ppm to dampen large burns.
    pub slope_ppm: u64,
}

impl RedeemCurve {
    /// Price `units` of compute with a linear curve.
    pub fn redeem(&self, units: u64) -> u64 {
        let premium = self.base * self.slope_ppm / 1_000_000;
        self.base * units + premium * units
    }
}

/// Fee-funded backstop that guarantees redemption up to `reserve` units.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Backstop {
    pub reserve: u64,
}

impl Backstop {
    pub fn new(reserve: u64) -> Self {
        Self { reserve }
    }
    /// Top up the reserve using fee revenue.
    pub fn top_up(&mut self, amount: u64) {
        self.reserve += amount;
    }
}
