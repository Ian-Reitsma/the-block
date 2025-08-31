#![forbid(unsafe_code)]

use std::collections::HashMap;

/// Simple bridge contract tracking locked and minted balances.
#[derive(Default)]
pub struct Bridge {
    locked: HashMap<String, u64>,
    minted: HashMap<String, u64>,
}

impl Bridge {
    pub fn lock(&mut self, user: &str, amount: u64) {
        *self.locked.entry(user.to_string()).or_insert(0) += amount;
    }
    pub fn mint(&mut self, user: &str, amount: u64) {
        *self.minted.entry(user.to_string()).or_insert(0) += amount;
    }
    pub fn burn(&mut self, user: &str, amount: u64) -> bool {
        let entry = self.minted.entry(user.to_string()).or_insert(0);
        if *entry < amount {
            return false;
        }
        *entry -= amount;
        true
    }
    pub fn release(&mut self, user: &str, amount: u64) -> bool {
        let entry = self.locked.entry(user.to_string()).or_insert(0);
        if *entry < amount {
            return false;
        }
        *entry -= amount;
        true
    }
    pub fn locked(&self, user: &str) -> u64 {
        self.locked.get(user).copied().unwrap_or(0)
    }
    pub fn minted(&self, user: &str) -> u64 {
        self.minted.get(user).copied().unwrap_or(0)
    }
}
