use crate::compute_market::receipt::Receipt;
use blake3;
use credits::{CreditError, Ledger, Source};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sled::Tree;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

#[cfg(feature = "telemetry")]
use crate::telemetry::CREDIT_BURN_TOTAL;

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
    ledger: Ledger,
    ledger_path: PathBuf,
    applied: Tree,
    failures: Tree,
    roots: Tree,
    #[allow(dead_code)]
    min_fee_micros: u64,
    decay_lambda_per_hour: f64,
}

static GLOBAL: Lazy<Mutex<Option<Settlement>>> = Lazy::new(|| Mutex::new(None));

impl Settlement {
    pub fn init(path: &str, mode: SettleMode, min_fee_micros: u64, decay_lambda_per_hour: f64) {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open settle db: {e}"));
        let applied = db
            .open_tree("receipts_applied")
            .unwrap_or_else(|e| panic!("open applied: {e}"));
        let failures = db
            .open_tree("failures")
            .unwrap_or_else(|e| panic!("open failures: {e}"));
        let ledger_path = Path::new(path).join("credits.bin");
        let roots = db
            .open_tree("microshard_roots")
            .unwrap_or_else(|e| panic!("open roots: {e}"));
        let ledger = Ledger::load(&ledger_path).unwrap_or_else(|e| panic!("load ledger: {e}"));
        *GLOBAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Self {
            mode,
            ledger,
            ledger_path,
            applied,
            failures,
            roots,
            min_fee_micros,
            decay_lambda_per_hour,
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

    pub fn set_decay_lambda(lambda: f64) {
        Self::with(|s| s.decay_lambda_per_hour = lambda);
    }

    pub fn decay_lambda() -> f64 {
        Self::with(|s| s.decay_lambda_per_hour)
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
            s.ledger.set_balance(acct, amt);
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
        });
    }

    pub fn balance(acct: &str) -> u64 {
        Self::with(|s| s.ledger.balance(acct))
    }

    #[allow(unused_variables)]
    pub fn spend(provider: &str, sink: &str, amount: u64) -> Result<(), CreditError> {
        Self::with(|s| {
            s.ledger.spend(provider, amount)?;
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
            #[cfg(feature = "telemetry")]
            {
                CREDIT_BURN_TOTAL.with_label_values(&[sink]).inc_by(amount);
            }
            Ok(())
        })
    }

    pub fn accrue(provider: &str, event: &str, source: Source, amount: u64, expiry_days: u64) {
        Self::with(|s| {
            s.ledger.accrue_with(
                provider,
                event,
                source,
                amount,
                SystemTime::now(),
                expiry_days,
            );
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
        });
    }

    pub fn mode() -> SettleMode {
        Self::with(|s| s.mode)
    }

    fn apply_receipt(&mut self, r: &Receipt, height: u64) -> Result<(), ()> {
        let amount = r.quote_price;
        let key = r.idempotency_key;
        if self
            .applied
            .get(&key)
            .unwrap_or_else(|e| panic!("applied get: {e}"))
            .is_some()
        {
            return Ok(());
        }
        if self.ledger.spend(&r.buyer, amount).is_err() {
            let bytes = bincode::serialize(r).unwrap_or_else(|e| panic!("serialize receipt: {e}"));
            self.failures
                .insert(&key, bytes)
                .unwrap_or_else(|e| panic!("record failure: {e}"));
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
            return Err(());
        }
        let event = format!("settle:{}", hex::encode(key));
        self.ledger.accrue(&r.provider, &event, amount);
        self.ledger
            .save(&self.ledger_path)
            .unwrap_or_else(|e| panic!("save ledger: {e}"));
        self.applied
            .insert(
                &key,
                bincode::serialize(&height).unwrap_or_else(|e| panic!("serialize height: {e}")),
            )
            .unwrap_or_else(|e| panic!("record applied: {e}"));
        self.applied
            .flush()
            .unwrap_or_else(|e| panic!("flush applied: {e}"));
        #[cfg(feature = "telemetry")]
        crate::telemetry::SETTLE_APPLIED_TOTAL.inc();
        Ok(())
    }

    pub fn tick(height: u64, receipts: &[Receipt]) {
        Self::with(|s| {
            s.ledger
                .decay_and_expire(s.decay_lambda_per_hour, SystemTime::now());
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
            // post root for this batch
            let key = height.to_be_bytes();
            let root = blake3::hash(&key);
            s.roots
                .insert(&key, root.as_bytes())
                .unwrap_or_else(|e| panic!("record root: {e}"));
            s.roots
                .flush()
                .unwrap_or_else(|e| panic!("flush roots: {e}"));
        });
    }

    pub fn receipt_applied(key: &[u8; 32]) -> bool {
        Self::with(|s| {
            s.applied
                .get(key)
                .unwrap_or_else(|e| panic!("get applied: {e}"))
                .is_some()
        })
    }
}

impl Settlement {
    pub fn recent_roots(n: usize) -> Vec<String> {
        Self::with(|s| {
            s.roots
                .iter()
                .rev()
                .take(n)
                .filter_map(|res| res.ok())
                .map(|(_, v)| hex::encode(v.as_ref()))
                .collect()
        })
    }
}
