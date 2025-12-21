use foundation_serialization::{Deserialize, Serialize};

/// Treasury account that accumulates a percentage of block subsidies.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TreasuryState {
    /// Total CT collected in the treasury.
    pub balance: u64,
}

impl TreasuryState {
    /// Collect `percent` of `reward` into the treasury, returning the remainder.
    pub fn collect(&mut self, reward: u64, percent: u64) -> u64 {
        let take = reward.saturating_mul(percent) / 100;
        self.balance = self.balance.saturating_add(take);
        reward - take
    }
}
