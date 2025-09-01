use crate::compute_market::receipt::Receipt;
use crate::gateway::read_receipt::ReadReceipt;
use blake3;
use credits::{CreditError, Ledger, Source};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sled::Tree;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    db: sled::Db,
    mode: SettleMode,
    ledger: Ledger,
    ledger_path: PathBuf,
    applied: Tree,
    failures: Tree,
    roots: Tree,
    #[allow(dead_code)]
    min_fee_micros: u64,
    decay_lambda_per_hour: f64,
    dispute_window_epochs: u64,
    receipts_dir: PathBuf,
    daily_payout_cap: u64,
    payouts_today: HashMap<String, (u64, u64)>,
}

static GLOBAL: Lazy<Mutex<Option<Settlement>>> = Lazy::new(|| Mutex::new(None));
static AUDITOR: Lazy<Mutex<Option<thread::JoinHandle<()>>>> = Lazy::new(|| Mutex::new(None));
static AUDIT_RUN: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(true));

impl Settlement {
    pub fn init(
        path: &str,
        mode: SettleMode,
        min_fee_micros: u64,
        decay_lambda_per_hour: f64,
        dispute_window_epochs: u64,
    ) {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open settle db: {e}"));
        let applied = db
            .open_tree("receipts_applied")
            .unwrap_or_else(|e| panic!("open applied: {e}"));
        let failures = db
            .open_tree("failures")
            .unwrap_or_else(|e| panic!("open failures: {e}"));
        let ledger_path = Path::new(path).join("credits.bin");
        let receipts_dir = Path::new(path).join("receipts");
        let _ = std::fs::create_dir_all(receipts_dir.join("pending"));
        let _ = std::fs::create_dir_all(receipts_dir.join("finalized"));
        let roots = db
            .open_tree("microshard_roots")
            .unwrap_or_else(|e| panic!("open roots: {e}"));
        let ledger = Ledger::load(&ledger_path).unwrap_or_else(|e| panic!("load ledger: {e}"));
        *GLOBAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Self {
            db,
            mode,
            ledger,
            ledger_path,
            applied,
            failures,
            roots,
            min_fee_micros,
            decay_lambda_per_hour,
            dispute_window_epochs,
            receipts_dir,
            daily_payout_cap: u64::MAX,
            payouts_today: HashMap::new(),
        });
        if let Some(ms) = std::env::var("TB_SETTLE_AUDIT_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            AUDIT_RUN.store(true, Ordering::Relaxed);
            let handle = thread::spawn(move || {
                let interval = Duration::from_millis(ms);
                while AUDIT_RUN.load(Ordering::Relaxed) {
                    let reports = Self::audit();
                    if !reports.is_empty() {
                        let dir = Self::with(|s| s.receipts_dir.clone());
                        let path = dir.join("audit_latest.json");
                        if let Ok(json) = serde_json::to_vec(&reports) {
                            let _ = std::fs::write(&path, json);
                        }
                    }
                    thread::sleep(interval);
                }
            });
            *AUDITOR.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
        }
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
        AUDIT_RUN.store(false, Ordering::Relaxed);
        if let Some(handle) = AUDITOR.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = handle.join();
        }
        let mut guard = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = guard.take() {
            let _ = s.db.flush();
        }
    }

    pub fn set_balance(acct: &str, amt: u64) {
        Self::with(|s| {
            s.ledger.set_balance(acct, amt);
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
        });
    }

    pub fn seed_read_pool(amount: u64) {
        Self::with(|s| {
            s.ledger.seed_read_pool(amount);
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
        });
    }

    pub fn meter(provider: &str) -> HashMap<Source, (u64, u64)> {
        Self::with(|s| {
            s.ledger
                .meter(provider, s.decay_lambda_per_hour, SystemTime::now())
        })
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

    pub fn penalize_sla(provider: &str, amount: u64) -> Result<(), CreditError> {
        Self::with(|s| {
            s.ledger.spend(provider, amount)?;
            #[cfg(feature = "telemetry")]
            crate::telemetry::INDUSTRIAL_REJECTED_TOTAL
                .with_label_values(&["SLA"])
                .inc();
            s.ledger
                .save(&s.ledger_path)
                .unwrap_or_else(|e| panic!("save ledger: {e}"));
            Ok(())
        })
    }

    pub fn set_daily_payout_cap(cap: u64) {
        Self::with(|s| s.daily_payout_cap = cap);
    }

    pub fn accrue(provider: &str, event: &str, source: Source, amount: u64, expiry_days: u64) {
        Self::with(|s| {
            let now = SystemTime::now();
            if source == Source::Read {
                let _ = s
                    .ledger
                    .issue_read(provider, event, amount, now, expiry_days);
            } else {
                s.ledger
                    .accrue_with(provider, event, source, amount, now, expiry_days);
            }
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
        let mut to_credit = amount;
        if self.daily_payout_cap < u64::MAX {
            let today = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                / 86_400;
            let entry = self
                .payouts_today
                .entry(r.provider.clone())
                .or_insert((today, 0));
            if entry.0 != today {
                *entry = (today, 0);
            }
            if entry.1 >= self.daily_payout_cap {
                to_credit = 0;
            } else if entry.1 + to_credit > self.daily_payout_cap {
                to_credit = self.daily_payout_cap - entry.1;
            }
            entry.1 += to_credit;
            if to_credit < amount {
                #[cfg(feature = "telemetry")]
                crate::telemetry::PAYOUT_CAP_HITS_TOTAL
                    .with_label_values(&[&r.provider])
                    .inc();
            }
        }
        if to_credit > 0 {
            self.ledger.accrue(&r.provider, &event, to_credit);
        }
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
            // persist receipts for dispute window
            if !receipts.is_empty() {
                let pending = s.receipts_dir.join("pending").join(height.to_string());
                let bytes = bincode::serialize(receipts)
                    .unwrap_or_else(|e| panic!("serialize receipts: {e}"));
                std::fs::write(&pending, bytes).unwrap_or_else(|e| panic!("write pending: {e}"));
            }
            if height >= s.dispute_window_epochs {
                let finalize_h = height - s.dispute_window_epochs;
                let pending = s.receipts_dir.join("pending").join(finalize_h.to_string());
                if let Ok(bytes) = std::fs::read(&pending) {
                    if let Ok(list) = bincode::deserialize::<Vec<Receipt>>(&bytes) {
                        if let SettleMode::Real = s.mode {
                            for r in &list {
                                let _ = s.apply_receipt(r, finalize_h);
                            }
                        }
                    }
                    let finalized = s
                        .receipts_dir
                        .join("finalized")
                        .join(finalize_h.to_string());
                    let _ = std::fs::rename(pending, finalized);
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

    pub fn dispute(height: u64, key: [u8; 32]) -> bool {
        Self::with(|s| {
            if s.applied
                .get(&key)
                .unwrap_or_else(|e| panic!("applied get: {e}"))
                .is_some()
            {
                let event = format!("settle:{}", hex::encode(key));
                s.ledger.rollback_by_event(&event);
                let _ = s.applied.remove(&key);
                s.ledger
                    .save(&s.ledger_path)
                    .unwrap_or_else(|e| panic!("save ledger: {e}"));
                return true;
            }
            let path = s.receipts_dir.join("pending").join(height.to_string());
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(mut list) = bincode::deserialize::<Vec<Receipt>>(&bytes) {
                    if let Some(pos) = list.iter().position(|r| r.idempotency_key == key) {
                        list.remove(pos);
                        let bytes =
                            bincode::serialize(&list).unwrap_or_else(|e| panic!("serialize: {e}"));
                        std::fs::write(&path, bytes)
                            .unwrap_or_else(|e| panic!("write pending: {e}"));
                        return true;
                    }
                }
            }
            false
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

    /// Audit pending receipt checkpoints and verify idempotency keys.
    pub fn audit() -> Vec<AuditSummary> {
        Self::with(|s| {
            let mut out = Vec::new();
            if let Ok(entries) = std::fs::read_dir(s.receipts_dir.join("pending")) {
                for ent in entries.flatten() {
                    if let Ok(epoch) = ent.file_name().to_string_lossy().parse::<u64>() {
                        if let Ok(bytes) = std::fs::read(ent.path()) {
                            if let Ok(list) = bincode::deserialize::<Vec<Receipt>>(&bytes) {
                                let mut invalid = 0;
                                for r in &list {
                                    let recompute = Receipt::new(
                                        r.job_id.clone(),
                                        r.buyer.clone(),
                                        r.provider.clone(),
                                        r.quote_price,
                                        r.dry_run,
                                    );
                                    if recompute.idempotency_key != r.idempotency_key {
                                        invalid += 1;
                                    }
                                }
                                let summary = AuditSummary {
                                    epoch,
                                    receipts: list.len() as u64,
                                    invalid: invalid as u64,
                                };
                                #[cfg(feature = "telemetry")]
                                if summary.invalid > 0 {
                                    crate::telemetry::SETTLE_AUDIT_MISMATCH_TOTAL
                                        .inc_by(summary.invalid);
                                }
                                out.push(summary);
                            }
                        }
                    }
                }
            }
            out
        })
    }
}

/// Summary of receipt verification for an epoch.
#[derive(Serialize, Deserialize, Debug)]
pub struct AuditSummary {
    pub epoch: u64,
    pub receipts: u64,
    pub invalid: u64,
}

pub fn submit_anchor(root: &[u8]) {
    Settlement::with(|s| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_be_bytes();
        let _ = s.roots.insert(ts, root);
    });
}

pub fn confirm_anchor(root: &[u8]) {
    let base = std::env::var("TB_GATEWAY_RECEIPTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("receipts"))
        .join("read");
    if let Ok(entries) = std::fs::read_dir(&base) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            if name.ends_with(".root") {
                let epoch = name.trim_end_matches(".root");
                let read_root = base.join(&name);
                if let Ok(read_hex) = std::fs::read_to_string(&read_root) {
                    if let Ok(read_bytes) = hex::decode(read_hex.trim()) {
                        let exec_root = base
                            .parent()
                            .unwrap_or_else(|| Path::new("."))
                            .join("exec")
                            .join(format!("{}.root", epoch));
                        if let Ok(exec_hex) = std::fs::read_to_string(&exec_root) {
                            if let Ok(exec_bytes) = hex::decode(exec_hex.trim()) {
                                let mut h = blake3::Hasher::new();
                                h.update(&read_bytes);
                                h.update(&exec_bytes);
                                if h.finalize().as_bytes() == root {
                                    let final_root = base.join(format!("{}.root.final", epoch));
                                    let _ = std::fs::rename(&read_root, &final_root);
                                    let dir = base.join(epoch);
                                    let final_dir = base.join(format!("{}.final", epoch));
                                    let _ = std::fs::rename(&dir, &final_dir);
                                    if let Ok(files) = std::fs::read_dir(&final_dir) {
                                        for f in files.flatten() {
                                            if f.path().extension().and_then(|s| s.to_str())
                                                == Some("cbor")
                                            {
                                                if let Ok(bytes) = std::fs::read(f.path()) {
                                                    if let Ok(rr) =
                                                        serde_cbor::from_slice::<ReadReceipt>(
                                                            &bytes,
                                                        )
                                                    {
                                                        let ev = format!(
                                                            "read:{}:{}",
                                                            epoch,
                                                            f.file_name().to_string_lossy()
                                                        );
                                                        crate::credits::issuance::issue_read(
                                                            &rr.provider_id,
                                                            "global",
                                                            &ev,
                                                            rr.bytes_served,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
