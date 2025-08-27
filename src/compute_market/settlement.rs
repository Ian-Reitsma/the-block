use crate::compute_market::receipt::Receipt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sled::{transaction::TransactionError, Tree};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettleMode {
    DryRun,
    Armed { activate_at: u64 },
    Real,
}

impl Serialize for SettleMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SettleMode::DryRun => serializer.serialize_str("dryrun"),
            SettleMode::Real => serializer.serialize_str("real"),
            SettleMode::Armed { .. } => serializer.serialize_str("armed"),
        }
    }
}

impl<'de> Deserialize<'de> for SettleMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "dryrun" => Ok(SettleMode::DryRun),
            "real" => Ok(SettleMode::Real),
            "armed" => Ok(SettleMode::Armed { activate_at: 0 }),
            _ => Err(serde::de::Error::custom("invalid settle mode")),
        }
    }
}

pub struct Settlement {
    mode: SettleMode,
    accounts: Tree,
    applied: Tree,
    failures: Tree,
    #[allow(dead_code)]
    min_fee_micros: u64,
}

static GLOBAL: Lazy<Mutex<Option<Settlement>>> = Lazy::new(|| Mutex::new(None));

impl Settlement {
    pub fn init(path: &str, mode: SettleMode, min_fee_micros: u64) {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open settle db: {e}"));
        let accounts = db
            .open_tree("accounts")
            .unwrap_or_else(|e| panic!("open accounts: {e}"));
        let applied = db
            .open_tree("receipts_applied")
            .unwrap_or_else(|e| panic!("open applied: {e}"));
        let failures = db
            .open_tree("failures")
            .unwrap_or_else(|e| panic!("open failures: {e}"));
        *GLOBAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Self {
            mode,
            accounts,
            applied,
            failures,
            min_fee_micros,
        });
    }

    fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Settlement) -> R,
    {
        let mut guard = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let sett = guard
            .as_mut()
            .unwrap_or_else(|| panic!("settlement not initialized"));
        f(sett)
    }

    pub fn arm(delay: u64, current_height: u64) {
        Self::with(|s| {
            s.mode = SettleMode::Armed {
                activate_at: current_height + delay,
            };
            #[cfg(feature = "telemetry")]
            crate::telemetry::SETTLE_MODE_CHANGE_TOTAL
                .with_label_values(&["armed"])
                .inc();
        });
    }

    pub fn cancel_arm() {
        Self::with(|s| {
            s.mode = SettleMode::DryRun;
            #[cfg(feature = "telemetry")]
            crate::telemetry::SETTLE_MODE_CHANGE_TOTAL
                .with_label_values(&["dryrun"])
                .inc();
        });
    }

    pub fn back_to_dry_run(_reason: &str) {
        Self::cancel_arm();
    }

    pub fn shutdown() {
        *GLOBAL.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn set_balance(acct: &str, amt: u64) {
        Self::with(|s| {
            s.accounts
                .insert(
                    acct,
                    bincode::serialize(&amt).unwrap_or_else(|e| panic!("serialize balance: {e}")),
                )
                .unwrap_or_else(|e| panic!("set balance: {e}"));
            s.accounts
                .flush()
                .unwrap_or_else(|e| panic!("flush balance: {e}"));
        });
    }

    pub fn balance(acct: &str) -> u64 {
        Self::with(|s| {
            s.accounts
                .get(acct)
                .unwrap_or_else(|e| panic!("get balance: {e}"))
                .map(|v| {
                    bincode::deserialize::<u64>(&v)
                        .unwrap_or_else(|e| panic!("deserialize balance: {e}"))
                })
                .unwrap_or(0)
        })
    }

    pub fn mode() -> SettleMode {
        Self::with(|s| s.mode)
    }

    fn apply_receipt(&mut self, r: &Receipt, height: u64) -> Result<(), ()> {
        use sled::transaction::Transactional;
        let amount = r.quote_price;
        let key = r.idempotency_key;
        let res =
            (&self.accounts, &self.applied, &self.failures).transaction(|(acc, app, fail)| {
                if app.get(&key)?.is_some() {
                    return Ok(());
                }
                let buyer_key = r.buyer.as_bytes();
                let provider_key = r.provider.as_bytes();
                let buyer_bal = acc
                    .get(buyer_key)?
                    .map(|v| {
                        bincode::deserialize::<u64>(&v)
                            .unwrap_or_else(|e| panic!("deserialize buyer balance: {e}"))
                    })
                    .unwrap_or(0);
                if buyer_bal < amount {
                    let bytes =
                        bincode::serialize(r).unwrap_or_else(|e| panic!("serialize receipt: {e}"));
                    fail.insert(&key, bytes)?;
                    return Err(sled::transaction::ConflictableTransactionError::Abort(()));
                }
                let prov_bal = acc
                    .get(provider_key)?
                    .map(|v| {
                        bincode::deserialize::<u64>(&v)
                            .unwrap_or_else(|e| panic!("deserialize provider balance: {e}"))
                    })
                    .unwrap_or(0);
                acc.insert(
                    buyer_key,
                    bincode::serialize(&(buyer_bal - amount))
                        .unwrap_or_else(|e| panic!("serialize debit: {e}")),
                )?;
                acc.insert(
                    provider_key,
                    bincode::serialize(&(prov_bal + amount))
                        .unwrap_or_else(|e| panic!("serialize credit: {e}")),
                )?;
                app.insert(
                    &key,
                    bincode::serialize(&height).unwrap_or_else(|e| panic!("serialize height: {e}")),
                )?;
                Ok(())
            });
        match res {
            Ok(_) => {
                self.accounts
                    .flush()
                    .unwrap_or_else(|e| panic!("flush accounts: {e}"));
                self.applied
                    .flush()
                    .unwrap_or_else(|e| panic!("flush applied: {e}"));
                #[cfg(feature = "telemetry")]
                crate::telemetry::SETTLE_APPLIED_TOTAL.inc();
                Ok(())
            }
            Err(TransactionError::Abort(_)) => {
                self.failures
                    .flush()
                    .unwrap_or_else(|e| panic!("flush failures: {e}"));
                #[cfg(feature = "telemetry")]
                crate::telemetry::SETTLE_FAILED_TOTAL
                    .with_label_values(&["insufficient_funds"])
                    .inc();
                self.mode = SettleMode::DryRun;
                #[cfg(feature = "telemetry")]
                crate::telemetry::SETTLE_MODE_CHANGE_TOTAL
                    .with_label_values(&["dryrun"])
                    .inc();
                Err(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn tick(height: u64, receipts: &[Receipt]) {
        Self::with(|s| {
            match s.mode {
                SettleMode::Armed { activate_at } => {
                    if height >= activate_at {
                        s.mode = SettleMode::Real;
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::SETTLE_MODE_CHANGE_TOTAL
                            .with_label_values(&["real"])
                            .inc();
                    }
                }
                _ => {}
            }
            if let SettleMode::Real = s.mode {
                for r in receipts {
                    let _ = s.apply_receipt(r, height);
                }
            }
        });
    }
}
