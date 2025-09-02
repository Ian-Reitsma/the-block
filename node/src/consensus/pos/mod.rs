use std::collections::HashMap;

use super::unl::Unl;

/// Simple staking ledger and validator manager for PoS.
///
/// The legacy implementation tracked a single stake value per validator.
/// Service-role staking requires maintaining separate balances per role
/// (gateway, storage, exec) while still exposing validator stake for the
/// finality gadget.  The ledger therefore maps an identity to a set of
/// role stakes.  The special role name `"validator"` carries the weight
/// used by the UNL and consensus.
#[derive(Default)]
pub struct PosState {
    /// id -> role -> bonded stake
    ledger: HashMap<String, HashMap<String, u64>>,
    unl: Unl,
}

impl PosState {
    /// Register a new validator with zero bonded stake.
    pub fn register(&mut self, id: String) {
        // `register` only concerns validator role; other service roles are
        // created lazily on first bond.
        if self.ledger.contains_key(&id) {
            return;
        }
        self.ledger.insert(id.clone(), HashMap::new());
        self.unl.add_validator(id, 0);
    }

    /// Bond stake to a role, increasing its weight.  When the role is
    /// `"validator"` the UNL is updated to reflect the new validator weight.
    pub fn bond(&mut self, id: &str, role: &str, amount: u64) {
        let roles = self
            .ledger
            .entry(id.to_string())
            .or_insert_with(HashMap::new);
        let entry = roles.entry(role.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
        if role == "validator" {
            self.unl.add_validator(id.to_string(), *entry);
        }
    }

    /// Unbond stake from a role. Removes the role entry if it drops to zero
    /// and, for validator role, updates the UNL accordingly.
    pub fn unbond(&mut self, id: &str, role: &str, amount: u64) {
        if let Some(roles) = self.ledger.get_mut(id) {
            if let Some(entry) = roles.get_mut(role) {
                *entry = entry.saturating_sub(amount);
                if *entry == 0 {
                    roles.remove(role);
                }
                if role == "validator" {
                    if let Some(v) = roles.get(role) {
                        self.unl.add_validator(id.to_string(), *v);
                    } else {
                        self.unl.remove_validator(id);
                    }
                }
            }
            if roles.is_empty() {
                self.ledger.remove(id);
            }
        }
    }

    /// Slash stake from a role as a penalty.
    pub fn slash(&mut self, id: &str, role: &str, amount: u64) {
        self.unbond(id, role, amount);
    }

    /// Get stake for a specific role.
    pub fn stake_of(&self, id: &str, role: &str) -> u64 {
        self.ledger
            .get(id)
            .and_then(|r| r.get(role))
            .copied()
            .unwrap_or(0)
    }

    /// Snapshot the current UNL for finality gadget.
    pub fn unl(&self) -> Unl {
        self.unl.clone()
    }
}
