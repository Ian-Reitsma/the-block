use std::collections::{HashMap, HashSet};

use super::unl::Unl;

/// Simple finality gadget counting stake votes.
pub struct FinalityGadget {
    unl: Unl,
    votes: HashMap<String, String>,
    equivocations: HashSet<String>,
    finalized: Option<String>,
}

impl FinalityGadget {
    /// Create a new gadget with the given UNL snapshot.
    pub fn new(unl: Unl) -> Self {
        Self {
            unl,
            votes: HashMap::new(),
            equivocations: HashSet::new(),
            finalized: None,
        }
    }

    /// Cast a vote for a block hash. Returns true if the block becomes finalized.
    pub fn vote(&mut self, validator: &str, block_hash: &str) -> bool {
        if self.equivocations.contains(validator) {
            return self.finalized.as_deref() == Some(block_hash);
        }

        match self.votes.get(validator) {
            Some(existing) if existing == block_hash => {}
            Some(_) => {
                // Conflicting vote: mark validator faulty and discard stake.
                self.votes.remove(validator);
                self.equivocations.insert(validator.to_string());
                return false;
            }
            None => {
                self.votes
                    .insert(validator.to_string(), block_hash.to_string());
            }
        }
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
        self.equivocations.clear();
        self.finalized = None;
    }

    /// Mutable access to the underlying UNL for governance updates.
    pub fn unl_mut(&mut self) -> &mut Unl {
        &mut self.unl
    }

    /// Snapshot the current voting state for auditability.
    pub fn snapshot(&self) -> FinalitySnapshot {
        FinalitySnapshot {
            finalized: self.finalized.clone(),
            votes: self.votes.clone(),
            equivocations: self.equivocations.clone(),
            total_stake: self.unl.total_stake(),
        }
    }
}

/// Captures a deterministic view of the gadget state for testing and telemetry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalitySnapshot {
    pub finalized: Option<String>,
    pub votes: HashMap<String, String>,
    pub equivocations: HashSet<String>,
    pub total_stake: u64,
}
