#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Simple epoch-based liquidity mining reward distributor.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LiquidityReward {
    pub epoch: u64,
    pub reward_per_epoch: u64,
}

impl LiquidityReward {
    /// Distribute rewards for the current epoch based on pool shares.
    pub fn distribute(&mut self, shares: &BTreeMap<String, u128>) -> BTreeMap<String, u64> {
        self.epoch += 1;
        let total: u128 = shares.values().sum();
        if total == 0 {
            return BTreeMap::new();
        }
        shares
            .iter()
            .map(|(p, s)| {
                let amt = (self.reward_per_epoch as u128 * *s / total) as u64;
                (p.clone(), amt)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_rewards() {
        let mut lr = LiquidityReward {
            epoch: 0,
            reward_per_epoch: 100,
        };
        let mut shares = BTreeMap::new();
        shares.insert("a".into(), 50u128);
        shares.insert("b".into(), 50u128);
        let dist = lr.distribute(&shares);
        assert_eq!(dist.get("a"), Some(&50));
        assert_eq!(dist.get("b"), Some(&50));
    }
}
