use crypto_suite::hashing::blake3;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::simple_db;
use crate::simple_db::{names, SimpleDb, SimpleDbBatch};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    COMPUTE_SLA_VIOLATIONS_TOTAL, SETTLE_APPLIED_TOTAL, SETTLE_FAILED_TOTAL,
    SETTLE_MODE_CHANGE_TOTAL, SLASHING_BURN_CT_TOTAL,
};
use bincode;
use ledger::utxo_account::AccountLedger;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
#[cfg(feature = "telemetry")]
use tracing::error;

const AUDIT_CAP: usize = 256;
const ROOT_HISTORY: usize = 32;

const KEY_LEDGER_CT: &str = "ledger_ct";
const KEY_LEDGER_IT: &str = "ledger_it";
const KEY_MODE: &str = "mode";
const KEY_METADATA: &str = "metadata";
const KEY_AUDIT: &str = "audit_log";
const KEY_ROOTS: &str = "recent_roots";
const KEY_NEXT_SEQ: &str = "next_seq";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettleMode {
    DryRun,
    Armed { activate_at: u64 },
    Real,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Metadata {
    armed_requested_height: Option<u64>,
    armed_delay: Option<u64>,
    last_cancel_reason: Option<String>,
    last_anchor_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecord {
    pub sequence: u64,
    pub timestamp: u64,
    pub entity: String,
    pub memo: String,
    pub delta_ct: i64,
    pub delta_it: i64,
    pub balance_ct: u64,
    pub balance_it: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BalanceSnapshot {
    pub provider: String,
    pub ct: u64,
    pub industrial: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SettlementEngineInfo {
    pub engine: String,
    pub legacy_mode: bool,
}

struct SettlementState {
    db: SimpleDb,
    base: PathBuf,
    mode: SettleMode,
    metadata: Metadata,
    ct: AccountLedger,
    it: AccountLedger,
    audit: VecDeque<AuditRecord>,
    roots: VecDeque<[u8; 32]>,
    next_seq: u64,
}

impl SettlementState {
    fn new(base: PathBuf, mut mode: SettleMode, db: SimpleDb) -> Self {
        let ct = load_or_default::<AccountLedger, _>(&db, KEY_LEDGER_CT, AccountLedger::new);
        let it = load_or_default::<AccountLedger, _>(&db, KEY_LEDGER_IT, AccountLedger::new);
        let stored_mode = load_or_default::<SettleMode, _>(&db, KEY_MODE, || mode);
        mode = stored_mode;
        let metadata = load_or_default::<Metadata, _>(&db, KEY_METADATA, Metadata::default);
        let audit = load_or_default::<VecDeque<AuditRecord>, _>(&db, KEY_AUDIT, VecDeque::new);
        let roots = load_or_default::<VecDeque<[u8; 32]>, _>(&db, KEY_ROOTS, VecDeque::new);
        let next_seq = load_or_default::<u64, _>(&db, KEY_NEXT_SEQ, || 0u64);
        Self {
            db,
            base,
            mode,
            metadata,
            ct,
            it,
            audit,
            roots,
            next_seq,
        }
    }

    fn record_event(&mut self, entity: &str, memo: &str, delta_ct: i64, delta_it: i64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (balance_ct, balance_it) = self.balance_split(entity);
        let record = AuditRecord {
            sequence: self.next_seq,
            timestamp,
            entity: entity.to_string(),
            memo: memo.to_string(),
            delta_ct,
            delta_it,
            balance_ct,
            balance_it,
            anchor: None,
        };
        self.next_seq = self.next_seq.wrapping_add(1);
        if self.audit.len() >= AUDIT_CAP {
            self.audit.pop_front();
        }
        self.audit.push_back(record);
        self.update_root();
    }

    fn push_anchor_record(&mut self, hash_hex: String) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let record = AuditRecord {
            sequence: self.next_seq,
            timestamp,
            entity: "__anchor__".to_string(),
            memo: "anchor".to_string(),
            delta_ct: 0,
            delta_it: 0,
            balance_ct: 0,
            balance_it: 0,
            anchor: Some(hash_hex),
        };
        self.next_seq = self.next_seq.wrapping_add(1);
        if self.audit.len() >= AUDIT_CAP {
            self.audit.pop_front();
        }
        self.audit.push_back(record);
    }

    fn update_root(&mut self) {
        let root = compute_root(&self.ct, &self.it);
        if self.roots.back().copied() == Some(root) {
            return;
        }
        if self.roots.len() >= ROOT_HISTORY {
            self.roots.pop_front();
        }
        self.roots.push_back(root);
    }

    fn balance_split(&self, provider: &str) -> (u64, u64) {
        let ct = self.ct.balances.get(provider).copied().unwrap_or(0);
        let it = self.it.balances.get(provider).copied().unwrap_or(0);
        (ct, it)
    }

    fn balances(&self) -> Vec<BalanceSnapshot> {
        let mut providers: BTreeSet<&str> = self.ct.balances.keys().map(|s| s.as_str()).collect();
        providers.extend(self.it.balances.keys().map(|s| s.as_str()));
        providers
            .into_iter()
            .map(|p| {
                let (ct, industrial) = self.balance_split(p);
                BalanceSnapshot {
                    provider: p.to_string(),
                    ct,
                    industrial,
                }
            })
            .collect()
    }

    fn persist_all(&mut self) {
        let mut batch = self.db.batch();
        let mut encode = || -> io::Result<()> {
            enqueue_value(&mut batch, KEY_LEDGER_CT, &self.ct)?;
            enqueue_value(&mut batch, KEY_LEDGER_IT, &self.it)?;
            enqueue_value(&mut batch, KEY_MODE, &self.mode)?;
            enqueue_value(&mut batch, KEY_METADATA, &self.metadata)?;
            enqueue_value(&mut batch, KEY_AUDIT, &self.audit)?;
            enqueue_value(&mut batch, KEY_ROOTS, &self.roots)?;
            enqueue_value(&mut batch, KEY_NEXT_SEQ, &self.next_seq)?;
            Ok(())
        };

        if let Err(err) = encode().and_then(|_| self.db.write_batch(batch)) {
            #[cfg(feature = "telemetry")]
            {
                error!(?err, "persist settlement state");
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = err;
            }
        }
    }

    fn flush(&self) {
        let _ = self.db.flush();
    }
}

fn load_or_default<T, F>(db: &SimpleDb, key: &str, default: F) -> T
where
    T: DeserializeOwned,
    F: FnOnce() -> T,
{
    db.get(key)
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
        .unwrap_or_else(default)
}

fn enqueue_value<T: Serialize>(batch: &mut SimpleDbBatch, key: &str, value: &T) -> io::Result<()> {
    let bytes = bincode::serialize(value)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
    batch.put(key, &bytes)
}

fn compute_root(ct: &AccountLedger, it: &AccountLedger) -> [u8; 32] {
    let mut providers: BTreeSet<&str> = ct.balances.keys().map(|s| s.as_str()).collect();
    providers.extend(it.balances.keys().map(|s| s.as_str()));
    let mut hashes = Vec::new();
    for provider in providers {
        let ct = ct.balances.get(provider).copied().unwrap_or(0);
        let industrial = it.balances.get(provider).copied().unwrap_or(0);
        let mut hasher = blake3::Hasher::new();
        hasher.update(provider.as_bytes());
        hasher.update(&ct.to_le_bytes());
        hasher.update(&industrial.to_le_bytes());
        hashes.push(hasher.finalize());
    }
    hashes.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut root = blake3::Hash::from([0u8; 32]);
    for h in hashes {
        let mut hasher = blake3::Hasher::new();
        hasher.update(root.as_bytes());
        hasher.update(h.as_bytes());
        root = hasher.finalize();
    }
    *root.as_bytes()
}

static STATE: Lazy<Mutex<Option<SettlementState>>> = Lazy::new(|| Mutex::new(None));

fn with_state_mut<R>(f: impl FnOnce(&mut SettlementState) -> R) -> R {
    let mut guard = STATE.lock();
    let state = guard
        .as_mut()
        .expect("Settlement::init must be called before use");
    f(state)
}

fn with_state<R>(f: impl FnOnce(&SettlementState) -> R) -> R {
    let guard = STATE.lock();
    let state = guard
        .as_ref()
        .expect("Settlement::init must be called before use");
    f(state)
}

pub struct Settlement;

impl Settlement {
    pub fn init(path: &str, mode: SettleMode) {
        Self::init_with_factory(path, mode, SimpleDb::open_named);
    }

    pub fn init_with_factory<F>(path: &str, mode: SettleMode, factory: F)
    where
        F: Fn(&str, &str) -> SimpleDb,
    {
        {
            let mut guard = STATE.lock();
            if let Some(state) = guard.as_mut() {
                state.persist_all();
                state.flush();
            }
            *guard = None;
        }

        let base = if path.is_empty() {
            tempfile::tempdir()
                .expect("create settlement tempdir")
                .into_path()
        } else {
            PathBuf::from(path)
        };
        fs::create_dir_all(&base).unwrap_or_else(|e| panic!("create settlement dir: {e}"));
        let db_path = base.join("compute_settlement.db");
        let db_path_str = db_path
            .to_str()
            .unwrap_or_else(|| panic!("non-utf8 settlement db path: {}", db_path.display()));
        let mut state =
            SettlementState::new(base, mode, factory(names::COMPUTE_SETTLEMENT, db_path_str));
        state.persist_all();
        state.flush();
        *STATE.lock() = Some(state);
    }

    pub fn shutdown() {
        let mut guard = STATE.lock();
        if let Some(state) = guard.as_mut() {
            state.persist_all();
            state.flush();
        }
        *guard = None;
    }

    pub fn penalize_sla(provider: &str, amount: u64) -> Result<(), ()> {
        #[cfg(feature = "telemetry")]
        let _span = crate::log_context!(provider = provider);
        with_state_mut(|state| match state.ct.debit(provider, amount) {
            Ok(_) => {
                state.record_event(provider, "penalize_sla", -(amount as i64), 0);
                #[cfg(feature = "telemetry")]
                {
                    SLASHING_BURN_CT_TOTAL.inc_by(amount);
                    COMPUTE_SLA_VIOLATIONS_TOTAL
                        .with_label_values(&[provider])
                        .inc();
                    SETTLE_APPLIED_TOTAL.inc();
                }
                state.persist_all();
                Ok(())
            }
            Err(_) => {
                #[cfg(feature = "telemetry")]
                SETTLE_FAILED_TOTAL.with_label_values(&["penalize"]).inc();
                Err(())
            }
        })
    }

    pub fn accrue(provider: &str, event: &str, amount: u64) {
        with_state_mut(|state| {
            state.ct.deposit(provider, amount);
            state.record_event(provider, event, amount as i64, 0);
            #[cfg(feature = "telemetry")]
            SETTLE_APPLIED_TOTAL.inc();
            state.persist_all();
        });
    }

    pub fn accrue_split(provider: &str, ct: u64, it: u64) {
        with_state_mut(|state| {
            state.ct.deposit(provider, ct);
            state.it.deposit(provider, it);
            state.record_event(provider, "accrue_split", ct as i64, it as i64);
            #[cfg(feature = "telemetry")]
            SETTLE_APPLIED_TOTAL.inc();
            state.persist_all();
        });
    }

    pub fn submit_anchor(anchor: &[u8]) {
        let hash = blake3::hash(anchor).to_hex().to_string();
        let line = serde_json::json!({
            "kind": "compute_anchor",
            "hash": hash,
        })
        .to_string();
        with_state_mut(|state| {
            state.metadata.last_anchor_hex = Some(hash.clone());
            state.push_anchor_record(hash.clone());
            state.persist_all();
            if let Err(_err) = state::append_audit(Path::new(&state.base), &line) {
                #[cfg(feature = "telemetry")]
                {
                    error!(?_err, "append compute anchor audit");
                }
            }
        });
    }

    pub fn balance(provider: &str) -> u64 {
        with_state(|state| state.ct.balances.get(provider).copied().unwrap_or(0))
    }

    pub fn balance_split(provider: &str) -> (u64, u64) {
        with_state(|state| state.balance_split(provider))
    }

    pub fn balances() -> Vec<BalanceSnapshot> {
        with_state(|state| state.balances())
    }

    pub fn engine_info() -> SettlementEngineInfo {
        let engine = with_state(|state| state.db.backend_name().to_string());
        SettlementEngineInfo {
            engine,
            legacy_mode: simple_db::legacy_mode(),
        }
    }

    pub fn spend(provider: &str, event: &str, amount: u64) -> Result<(), ()> {
        with_state_mut(|state| match state.ct.debit(provider, amount) {
            Ok(_) => {
                state.record_event(provider, event, -(amount as i64), 0);
                #[cfg(feature = "telemetry")]
                SETTLE_APPLIED_TOTAL.inc();
                state.persist_all();
                Ok(())
            }
            Err(_) => {
                #[cfg(feature = "telemetry")]
                SETTLE_FAILED_TOTAL.with_label_values(&["spend"]).inc();
                Err(())
            }
        })
    }

    pub fn refund_split(buyer: &str, ct: u64, it: u64) {
        with_state_mut(|state| {
            state.ct.deposit(buyer, ct);
            state.it.deposit(buyer, it);
            state.record_event(buyer, "refund_split", ct as i64, it as i64);
            #[cfg(feature = "telemetry")]
            SETTLE_APPLIED_TOTAL.inc();
            state.persist_all();
        });
    }

    pub fn mode() -> SettleMode {
        with_state(|state| state.mode)
    }

    pub fn arm(delay: u64, current_height: u64) {
        with_state_mut(|state| {
            let activate_at = current_height.saturating_add(delay);
            state.mode = SettleMode::Armed { activate_at };
            state.metadata.armed_requested_height = Some(current_height);
            state.metadata.armed_delay = Some(delay);
            state.metadata.last_cancel_reason = None;
            #[cfg(feature = "telemetry")]
            SETTLE_MODE_CHANGE_TOTAL.with_label_values(&["armed"]).inc();
            state.persist_all();
        });
    }

    pub fn cancel_arm() {
        with_state_mut(|state| {
            state.mode = SettleMode::DryRun;
            state.metadata.armed_requested_height = None;
            state.metadata.armed_delay = None;
            #[cfg(feature = "telemetry")]
            SETTLE_MODE_CHANGE_TOTAL
                .with_label_values(&["dryrun"])
                .inc();
            state.persist_all();
        });
    }

    pub fn back_to_dry_run(reason: &str) {
        with_state_mut(|state| {
            state.mode = SettleMode::DryRun;
            state.metadata.last_cancel_reason = Some(reason.to_string());
            state.metadata.armed_requested_height = None;
            state.metadata.armed_delay = None;
            #[cfg(feature = "telemetry")]
            SETTLE_MODE_CHANGE_TOTAL
                .with_label_values(&["dryrun"])
                .inc();
            state.persist_all();
        });
    }

    pub fn audit() -> Vec<AuditRecord> {
        with_state(|state| state.audit.iter().cloned().collect())
    }

    pub fn recent_roots(n: usize) -> Vec<[u8; 32]> {
        with_state(|state| state.roots.iter().rev().take(n).cloned().collect())
    }
}

pub fn submit_anchor(anchor: &[u8]) {
    Settlement::submit_anchor(anchor);
}
