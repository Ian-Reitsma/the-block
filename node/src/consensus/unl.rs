use std::collections::HashMap;

/// Unique Node List with stake weights.
#[derive(Clone, Default)]
pub struct Unl {
    members: HashMap<String, u64>,
}

impl Unl {
    /// Add or update a validator and its stake. Governance layer should call this.
    pub fn add_validator(&mut self, id: String, stake: u64) {
        self.members.insert(id, stake);
    }

    /// Remove a validator by identifier.
    pub fn remove_validator(&mut self, id: &str) {
        self.members.remove(id);
    }

    /// Return total stake weight across all validators.
    #[must_use]
    pub fn total_stake(&self) -> u64 {
        self.members.values().copied().sum()
    }

    /// Get stake for a specific validator.
    #[must_use]
    pub fn stake_of(&self, id: &str) -> u64 {
        self.members.get(id).copied().unwrap_or(0)
    }

    /// Iterate over validators.
    pub fn members(&self) -> impl Iterator<Item = (&String, &u64)> {
        self.members.iter()
    }
}
