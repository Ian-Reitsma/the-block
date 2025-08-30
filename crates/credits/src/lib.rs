use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Identifier for a provider within the credit system.
pub type ProviderId = String;

/// Identifier for an event that may award credits.
pub type EventId = String;

/// Sources from which credits may be earned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    Uptime,
    LocalNetAssist,
    ProvenStorage,
    Civic,
}

impl Default for Source {
    fn default() -> Self {
        Source::Civic
    }
}

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
struct SourceEntry {
    amount: f64,
    expiry: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct ProviderEntry {
    sources: HashMap<Source, SourceEntry>,
    last_update: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Ledger {
    providers: HashMap<ProviderId, ProviderEntry>,
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

    /// Directly set the balance for `provider` to `amount` with no expiry.
    pub fn set_balance(&mut self, provider: &str, amount: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        let entry = self
            .providers
            .entry(provider.to_owned())
            .or_insert_with(ProviderEntry::default);
        entry.sources.clear();
        entry.sources.insert(
            Source::Civic,
            SourceEntry {
                amount: amount as f64,
                expiry: u64::MAX,
            },
        );
        entry.last_update = now;
    }

    /// Accrue `amount` credits to `provider` for `event` from `source` with an
    /// expiry window measured in days. Duplicate events are ignored.
    pub fn accrue_with(
        &mut self,
        provider: &str,
        event: &str,
        source: Source,
        amount: u64,
        now: SystemTime,
        expiry_days: u64,
    ) {
        if !self.processed.insert(event.to_owned()) {
            return;
        }
        let now_secs = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        let expiry = if expiry_days == u64::MAX {
            u64::MAX
        } else {
            now_secs + expiry_days * 24 * 60 * 60
        };
        let prov = self
            .providers
            .entry(provider.to_owned())
            .or_insert_with(ProviderEntry::default);
        let src = prov
            .sources
            .entry(source)
            .or_insert(SourceEntry { amount: 0.0, expiry });
        if expiry > src.expiry {
            src.expiry = expiry;
        }
        src.amount += amount as f64;
        prov.last_update = now_secs;
    }

    /// Accrue credits with default `Source::Civic` and no expiry.
    pub fn accrue(&mut self, provider: &str, event: &str, amount: u64) {
        self.accrue_with(
            provider,
            event,
            Source::Civic,
            amount,
            SystemTime::now(),
            u64::MAX,
        );
    }

    /// Spend `amount` credits from `provider` if available.
    pub fn spend(&mut self, provider: &str, amount: u64) -> Result<(), CreditError> {
        let prov = self
            .providers
            .entry(provider.to_owned())
            .or_insert_with(ProviderEntry::default);
        let total: f64 = prov.sources.values().map(|s| s.amount).sum();
        if total < amount as f64 {
            return Err(CreditError::Insufficient);
        }
        let mut remaining = amount as f64;
        for entry in prov.sources.values_mut() {
            if remaining <= 0.0 {
                break;
            }
            let take = remaining.min(entry.amount);
            entry.amount -= take;
            remaining -= take;
        }
        Ok(())
    }

    /// Return the balance for `provider`.
    pub fn balance(&self, provider: &str) -> u64 {
        self.providers
            .get(provider)
            .map(|p| p.sources.values().map(|s| s.amount).sum::<f64>() as u64)
            .unwrap_or(0)
    }

    /// Apply exponential decay to all balances and expire sources past their window.
    pub fn decay_and_expire(&mut self, lambda_per_hour: f64, now: SystemTime) {
        let now_secs = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        for prov in self.providers.values_mut() {
            let dt_hours = (now_secs.saturating_sub(prov.last_update)) as f64 / 3600.0;
            let decay = (-lambda_per_hour * dt_hours).exp();
            for src in prov.sources.values_mut() {
                src.amount *= decay;
                if now_secs >= src.expiry {
                    src.amount = 0.0;
                }
            }
            prov.last_update = now_secs;
        }
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
