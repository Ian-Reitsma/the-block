use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Identifier for a provider within the credit system.
pub type ProviderId = String;

/// Identifier for an event that may award credits.
pub type EventId = String;

#[derive(Debug, Error)]
pub enum CreditError {
    #[error("insufficient credits")]
    Insufficient,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("ser: {0}")]
    Ser(#[from] Box<bincode::ErrorKind>),
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Ledger {
    balances: HashMap<ProviderId, u64>,
    processed: HashSet<EventId>,
}

impl Ledger {
    /// Create an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a ledger from the given path, if it exists, otherwise create a new one.
    pub fn load(path: &Path) -> Result<Self, CreditError> {
        match fs::read(path) {
            Ok(bytes) => Ok(bincode::deserialize(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Persist the ledger to disk at the given path.
    pub fn save(&self, path: &Path) -> Result<(), CreditError> {
        let bytes = bincode::serialize(self)?;
        fs::write(path, bytes)?;
        Ok(())
    }

    /// Directly set the balance for `provider` to `amount`.
    pub fn set_balance(&mut self, provider: &str, amount: u64) {
        self.balances.insert(provider.to_owned(), amount);
    }

    /// Accrue `amount` credits to `provider` for `event`. Duplicate events are ignored.
    pub fn accrue(&mut self, provider: &str, event: &str, amount: u64) {
        if !self.processed.insert(event.to_owned()) {
            return;
        }
        *self.balances.entry(provider.to_owned()).or_default() += amount;
    }

    /// Spend `amount` credits from `provider` if available.
    pub fn spend(&mut self, provider: &str, amount: u64) -> Result<(), CreditError> {
        let bal = self.balances.entry(provider.to_owned()).or_default();
        if *bal < amount {
            return Err(CreditError::Insufficient);
        }
        *bal -= amount;
        Ok(())
    }

    /// Return the balance for `provider`.
    pub fn balance(&self, provider: &str) -> u64 {
        *self.balances.get(provider).unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accrual_and_spend_persist() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.bin");
        {
            let mut ledger = Ledger::new();
            ledger.accrue("prov1", "event1", 100);
            ledger.save(&path).unwrap();
        }
        {
            let mut ledger = Ledger::load(&path).unwrap();
            assert_eq!(ledger.balance("prov1"), 100);
            ledger.spend("prov1", 40).unwrap();
            ledger.save(&path).unwrap();
        }
        let ledger = Ledger::load(&path).unwrap();
        assert_eq!(ledger.balance("prov1"), 60);
    }

    #[test]
    fn duplicate_events_ignored() {
        let mut ledger = Ledger::new();
        ledger.accrue("p", "ev", 10);
        ledger.accrue("p", "ev", 10);
        assert_eq!(ledger.balance("p"), 10);
    }

    #[test]
    fn spending_checks_balance() {
        let mut ledger = Ledger::new();
        ledger.accrue("p", "e", 5);
        assert!(ledger.spend("p", 10).is_err());
        assert_eq!(ledger.balance("p"), 5);
    }
}
