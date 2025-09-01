use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Identifier for a provider within the credit system.
pub type ProviderId = String;

/// Identifier for an event that may award credits.
pub type EventId = String;

/// Sources from which credits may be earned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Source {
    Uptime,
    LocalNetAssist,
    ProvenStorage,
    #[default]
    Civic,
    Read,
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
    processed: HashMap<EventId, (ProviderId, f64)>,
    pub read_reward_pool: u64,
}

impl Ledger {
    /// Create an empty ledger.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            processed: HashMap::new(),
            read_reward_pool: 0,
        }
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

    pub fn seed_read_pool(&mut self, amount: u64) {
        self.read_reward_pool = self.read_reward_pool.saturating_add(amount);
    }

    /// Directly set the balance for `provider` to `amount` with no expiry.
    pub fn set_balance(&mut self, provider: &str, amount: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        let entry = self.providers.entry(provider.to_owned()).or_default();
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
        if self.processed.contains_key(event) {
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
        let prov = self.providers.entry(provider.to_owned()).or_default();
        let src = prov.sources.entry(source).or_insert(SourceEntry {
            amount: 0.0,
            expiry,
        });
        if expiry > src.expiry {
            src.expiry = expiry;
        }
        src.amount += amount as f64;
        prov.last_update = now_secs;
        self.processed
            .insert(event.to_owned(), (provider.to_owned(), amount as f64));
    }

    pub fn issue_read(
        &mut self,
        provider: &str,
        event: &str,
        amount: u64,
        now: SystemTime,
        expiry_days: u64,
    ) -> Result<(), CreditError> {
        if self.read_reward_pool < amount {
            return Err(CreditError::Insufficient);
        }
        self.read_reward_pool -= amount;
        self.accrue_with(provider, event, Source::Read, amount, now, expiry_days);
        Ok(())
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
        let prov = self.providers.entry(provider.to_owned()).or_default();
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

    /// Roll back a previously processed event, refunding credits.
    pub fn rollback_by_event(&mut self, event: &str) {
        if let Some((prov_id, amt)) = self.processed.remove(event) {
            if let Some(prov) = self.providers.get_mut(&prov_id) {
                if let Some(entry) = prov.sources.get_mut(&Source::Civic) {
                    entry.amount = (entry.amount - amt).max(0.0);
                }
            }
        }
    }

    /// Return the balance for `provider`.
    pub fn balance(&self, provider: &str) -> u64 {
        self.providers
            .get(provider)
            .map(|p| p.sources.values().map(|s| s.amount).sum::<f64>() as u64)
            .unwrap_or(0)
    }

    /// Return per-source balances and expiries without mutating the ledger.
    pub fn meter(
        &self,
        provider: &str,
        lambda_per_hour: f64,
        now: SystemTime,
    ) -> HashMap<Source, (u64, u64)> {
        let mut tmp = self.clone();
        tmp.decay_and_expire(lambda_per_hour, now);
        tmp.providers
            .get(provider)
            .map(|p| {
                p.sources
                    .iter()
                    .map(|(src, entry)| (*src, (entry.amount as u64, entry.expiry)))
                    .collect()
            })
            .unwrap_or_default()
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
