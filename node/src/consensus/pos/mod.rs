use std::collections::HashMap;

use super::unl::Unl;

/// Simple staking ledger and validator manager for PoS.
#[derive(Default)]
pub struct PosState {
    ledger: HashMap<String, u64>,
    unl: Unl,
}

impl PosState {
    /// Register a new validator with zero bonded stake.
    pub fn register(&mut self, id: String) {
        if self.ledger.contains_key(&id) {
            return;
        }
        self.ledger.insert(id.clone(), 0);
        self.unl.add_validator(id, 0);
    }

    /// Bond stake to a validator, increasing its weight.
    pub fn bond(&mut self, id: &str, amount: u64) {
        let entry = self.ledger.entry(id.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
        self.unl.add_validator(id.to_string(), *entry);
    }

    /// Unbond stake from a validator. Removes validator if stake drops to zero.
    pub fn unbond(&mut self, id: &str, amount: u64) {
        if let Some(entry) = self.ledger.get_mut(id) {
            *entry = entry.saturating_sub(amount);
            if *entry == 0 {
                self.ledger.remove(id);
                self.unl.remove_validator(id);
            } else {
                self.unl.add_validator(id.to_string(), *entry);
            }
        }
    }

    /// Slash stake from a validator as a penalty.
    pub fn slash(&mut self, id: &str, amount: u64) {
        self.unbond(id, amount);
    }

    /// Get stake for a validator.
    pub fn stake_of(&self, id: &str) -> u64 {
        self.ledger.get(id).copied().unwrap_or(0)
    }

    /// Snapshot the current UNL for finality gadget.
    pub fn unl(&self) -> Unl {
        self.unl.clone()
    }
}
