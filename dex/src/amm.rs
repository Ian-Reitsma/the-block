#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Constant-product automated market maker pool for CT/IT pairs.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct Pool {
    pub ct_reserve: u128,
    pub it_reserve: u128,
    pub total_shares: u128,
}

impl Pool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add liquidity to the pool returning minted pool shares.
    /// The initial LP share is the geometric mean of deposits; subsequent
    /// deposits mint shares proportional to existing reserves.
    pub fn add_liquidity(&mut self, ct: u128, it: u128) -> u128 {
        assert!(ct > 0 && it > 0, "zero liquidity");
        if self.total_shares == 0 {
            let share = (ct * it).integer_sqrt();
            self.ct_reserve = ct;
            self.it_reserve = it;
            self.total_shares = share;
            share
        } else {
            let share_ct = self.total_shares * ct / self.ct_reserve;
            let share_it = self.total_shares * it / self.it_reserve;
            let share = share_ct.min(share_it);
            self.ct_reserve += ct;
            self.it_reserve += it;
            self.total_shares += share;
            share
        }
    }

    /// Remove liquidity returning the withdrawn reserves.
    pub fn remove_liquidity(&mut self, share: u128) -> (u128, u128) {
        assert!(share <= self.total_shares);
        let ct = self.ct_reserve * share / self.total_shares;
        let it = self.it_reserve * share / self.total_shares;
        self.ct_reserve -= ct;
        self.it_reserve -= it;
        self.total_shares -= share;
        (ct, it)
    }

    /// Swap CT for IT; returns the IT amount received.
    pub fn swap_ct_for_it(&mut self, ct_in: u128) -> u128 {
        assert!(ct_in > 0);
        let k = self.ct_reserve * self.it_reserve;
        self.ct_reserve += ct_in;
        let new_it = k / self.ct_reserve;
        let it_out = self.it_reserve - new_it;
        self.it_reserve = new_it;
        it_out
    }

    /// Swap IT for CT; returns the CT amount received.
    pub fn swap_it_for_ct(&mut self, it_in: u128) -> u128 {
        assert!(it_in > 0);
        let k = self.ct_reserve * self.it_reserve;
        self.it_reserve += it_in;
        let new_ct = k / self.it_reserve;
        let ct_out = self.ct_reserve - new_ct;
        self.ct_reserve = new_ct;
        ct_out
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
        let k = p.ct_reserve * p.it_reserve;
        let _ = p.swap_ct_for_it(100);
        assert!(p.ct_reserve * p.it_reserve <= k);
    }

    #[test]
    fn add_remove_liquidity_roundtrip() {
        let mut p = Pool::new();
        let share = p.add_liquidity(500, 500);
        let (ct, it) = p.remove_liquidity(share);
        assert_eq!(ct, 500);
        assert_eq!(it, 500);
    }
}

