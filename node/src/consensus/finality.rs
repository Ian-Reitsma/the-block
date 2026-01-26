use std::collections::{BTreeMap, HashMap, HashSet};

use super::unl::Unl;
#[cfg(feature = "telemetry")]
use foundation_serialization::Serialize;

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
                self.try_finalize_all();
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
        self.try_finalize_all()
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
        let total_stake = self.unl.total_stake();
        let equivocated_stake = self.equivocated_stake();
        let effective_total = total_stake.saturating_sub(equivocated_stake);
        let mut vote_tallies = HashMap::new();
        for (validator, block_hash) in &self.votes {
            if self.equivocations.contains(validator) {
                continue;
            }
            let entry = vote_tallies.entry(block_hash.clone()).or_insert(0u64);
            *entry = entry.saturating_add(self.unl.stake_of(validator));
        }
        let mut equivocated_stakes = HashMap::new();
        for validator in &self.equivocations {
            equivocated_stakes.insert(validator.clone(), self.unl.stake_of(validator));
        }
        FinalitySnapshot {
            finalized: self.finalized.clone(),
            votes: self.votes.clone(),
            equivocations: self.equivocations.clone(),
            total_stake,
            equivocated_stake,
            effective_total_stake: effective_total,
            finality_threshold: Self::finality_threshold(effective_total),
            vote_tallies,
            equivocated_stake_by_validator: equivocated_stakes,
        }
    }

    fn equivocated_stake(&self) -> u64 {
        let mut total = 0u64;
        for validator in &self.equivocations {
            total = total.saturating_add(self.unl.stake_of(validator));
        }
        total
    }

    fn effective_total_stake(&self) -> u64 {
        let total = self.unl.total_stake();
        let equivocated = self.equivocated_stake();
        total.saturating_sub(equivocated)
    }

    fn finality_threshold(effective_total: u64) -> u64 {
        if effective_total == 0 {
            0
        } else {
            (effective_total.saturating_mul(2) + 2) / 3
        }
    }

    fn try_finalize_all(&mut self) -> bool {
        if self.finalized.is_some() {
            return false;
        }
        let effective_total = self.effective_total_stake();
        if effective_total == 0 {
            return false;
        }
        let threshold = Self::finality_threshold(effective_total);
        let mut tallies = BTreeMap::new();
        for (validator, block_hash) in &self.votes {
            if self.equivocations.contains(validator) {
                continue;
            }
            let entry = tallies.entry(block_hash).or_insert(0u64);
            *entry = entry.saturating_add(self.unl.stake_of(validator));
        }
        for (block_hash, stake_for) in tallies {
            if stake_for >= threshold {
                self.finalized = Some(block_hash.clone());
                return true;
            }
        }
        false
    }
}

/// Captures a deterministic view of the gadget state for testing and telemetry.
#[cfg_attr(feature = "telemetry", derive(Serialize))]
#[cfg_attr(
    feature = "telemetry",
    serde(crate = "foundation_serialization::serde")
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalitySnapshot {
    pub finalized: Option<String>,
    pub votes: HashMap<String, String>,
    pub equivocations: HashSet<String>,
    pub total_stake: u64,
    pub equivocated_stake: u64,
    pub effective_total_stake: u64,
    pub finality_threshold: u64,
    pub vote_tallies: HashMap<String, u64>,
    pub equivocated_stake_by_validator: HashMap<String, u64>,
}
