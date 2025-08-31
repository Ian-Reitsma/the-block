use std::collections::HashMap;

use super::unl::Unl;

/// Simple finality gadget counting stake votes.
pub struct FinalityGadget {
    unl: Unl,
    votes: HashMap<String, String>,
    finalized: Option<String>,
}

impl FinalityGadget {
    /// Create a new gadget with the given UNL snapshot.
    pub fn new(unl: Unl) -> Self {
        Self {
            unl,
            votes: HashMap::new(),
            finalized: None,
        }
    }

    /// Cast a vote for a block hash. Returns true if the block becomes finalized.
    pub fn vote(&mut self, validator: &str, block_hash: &str) -> bool {
        self.votes
            .insert(validator.to_string(), block_hash.to_string());
        if let Some(ref f) = self.finalized {
            return f == block_hash;
        }
        let mut stake_for = 0u64;
        for (v, h) in &self.votes {
            if h == block_hash {
                stake_for += self.unl.stake_of(v);
            }
        }
        if stake_for * 3 >= self.unl.total_stake() * 2 {
            self.finalized = Some(block_hash.to_string());
            true
        } else {
            false
        }
    }

    /// Current finalized block hash, if any.
    #[must_use]
    pub fn finalized(&self) -> Option<&str> {
        self.finalized.as_deref()
    }

    /// Roll back any finalized block and clear votes.
    pub fn rollback(&mut self) {
        self.votes.clear();
        self.finalized = None;
    }

    /// Mutable access to the underlying UNL for governance updates.
    pub fn unl_mut(&mut self) -> &mut Unl {
        &mut self.unl
    }
}
