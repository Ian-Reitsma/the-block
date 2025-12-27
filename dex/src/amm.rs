#![forbid(unsafe_code)]

use foundation_serialization::{Deserialize, Serialize};

/// Constant-product automated market maker pool for generic base/quote lanes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct Pool {
    pub base_reserve: u128,
    pub quote_reserve: u128,
    pub total_shares: u128,
}

impl Pool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add liquidity to the pool returning minted pool shares.
    /// The initial LP share is the geometric mean of deposits; subsequent
    /// deposits mint shares proportional to existing reserves.
    pub fn add_liquidity(&mut self, base: u128, quote: u128) -> u128 {
        assert!(base > 0 && quote > 0, "zero liquidity");
        if self.total_shares == 0 {
            let share = (base * quote).integer_sqrt();
            self.base_reserve = base;
            self.quote_reserve = quote;
            self.total_shares = share;
            share
        } else {
            let share_base = self.total_shares * base / self.base_reserve;
            let share_quote = self.total_shares * quote / self.quote_reserve;
            let share = share_base.min(share_quote);
            self.base_reserve += base;
            self.quote_reserve += quote;
            self.total_shares += share;
            share
        }
    }

    /// Remove liquidity returning the withdrawn reserves.
    pub fn remove_liquidity(&mut self, share: u128) -> (u128, u128) {
        assert!(share <= self.total_shares);
        let base = self.base_reserve * share / self.total_shares;
        let quote = self.quote_reserve * share / self.total_shares;
        self.base_reserve -= base;
        self.quote_reserve -= quote;
        self.total_shares -= share;
        (base, quote)
    }

    /// Swap base for quote; returns the quote amount received.
    pub fn swap_base_for_quote(&mut self, base_in: u128) -> u128 {
        assert!(base_in > 0);
        let k = self.base_reserve * self.quote_reserve;
        self.base_reserve += base_in;
        let new_quote = k / self.base_reserve;
        let quote_out = self.quote_reserve - new_quote;
        self.quote_reserve = new_quote;
        quote_out
    }

    /// Swap quote for base; returns the base amount received.
    pub fn swap_quote_for_base(&mut self, quote_in: u128) -> u128 {
        assert!(quote_in > 0);
        let k = self.base_reserve * self.quote_reserve;
        self.quote_reserve += quote_in;
        let new_base = k / self.quote_reserve;
        let base_out = self.base_reserve - new_base;
        self.base_reserve = new_base;
        base_out
    }
}

/// Integer square root using the Babylonian method.
trait IntegerSqrt {
    fn integer_sqrt(self) -> Self;
}

impl IntegerSqrt for u128 {
    fn integer_sqrt(self) -> Self {
        if self <= 1 {
            return self;
        }
        let mut x0 = self / 2;
        let mut x1 = (x0 + self / x0) / 2;
        while x1 < x0 {
            x0 = x1;
            x1 = (x0 + self / x0) / 2;
        }
        x0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_product_invariant() {
        let mut p = Pool::new();
        p.add_liquidity(1000, 1000);
        let k = p.base_reserve * p.quote_reserve;
        let _ = p.swap_base_for_quote(100);
        assert!(p.base_reserve * p.quote_reserve <= k);
    }

    #[test]
    fn add_remove_liquidity_roundtrip() {
        let mut p = Pool::new();
        let share = p.add_liquidity(500, 500);
        let (base, quote) = p.remove_liquidity(share);
        assert_eq!(base, 500);
        assert_eq!(quote, 500);
    }
}
