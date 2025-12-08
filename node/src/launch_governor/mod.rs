use crate::blockchain::process::validate_and_apply;
use crate::gateway::dns;
use crate::governance::Runtime;
use crate::governor_snapshot;
use crate::simple_db::{names, SimpleDb};
use crate::Blockchain;
use crypto_suite::hex;
use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use foundation_serialization::{Deserialize, Serialize};
use runtime::sync::CancellationToken;
use runtime::{self, sleep, JoinHandle};
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering as AtomicOrdering},
    Arc, Mutex,
};
use std::time::Duration;

#[allow(dead_code)]
const MAX_HISTORY: usize = 64;
const INTENT_PREFIX: &str = "intent/";
const LOG_LIMIT: usize = 64;

#[derive(Clone, Debug)]
pub struct GovernorConfig {
    pub enabled: bool,
    pub db_path: PathBuf,
    pub base_path: PathBuf,
    pub window_secs: u64,
}

impl GovernorConfig {
    pub fn from_env(node_path: &str) -> Self {
        let enabled = std::env::var("TB_GOVERNOR_ENABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let db_path = std::env::var("TB_GOVERNOR_DB").unwrap_or_else(|_| "governor_db".into());
        let base_path = Path::new(node_path).to_path_buf();
        let db_path = if Path::new(&db_path).is_absolute() {
            Path::new(&db_path).to_path_buf()
        } else {
            base_path.join(db_path)
        };
        let epoch_secs = crate::EPOCH_BLOCKS.max(1);
        let window_secs = std::env::var("TB_GOVERNOR_WINDOW_SECS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .map(|secs| secs.max(epoch_secs))
            .unwrap_or(epoch_secs * 2);
        Self {
            enabled,
            db_path,
            base_path,
            window_secs,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum GateAction {
    Enter,
    Exit,
    Rehearsal,
    Trade,
}

impl GateAction {
    fn as_str(&self) -> &'static str {
        match self {
            GateAction::Enter => "enter",
            GateAction::Exit => "exit",
            GateAction::Rehearsal => "rehearsal",
            GateAction::Trade => "trade",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct IntentMetrics {
    pub summary: JsonValue,
    pub raw: JsonValue,
}

impl Default for IntentMetrics {
    fn default() -> Self {
        Self {
            summary: JsonValue::Null,
            raw: JsonValue::Null,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum IntentState {
    Pending,
    Applied { epoch: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct IntentRecord {
    pub id: String,
    pub gate: String,
    pub action: GateAction,
    pub created_epoch: u64,
    pub epoch_apply: u64,
    pub params_patch: JsonValue,
    pub metrics: IntentMetrics,
    pub snapshot_hash_hex: String,
    pub state: IntentState,
}

impl IntentRecord {
    fn summary(&self) -> IntentSummary {
        IntentSummary {
            id: self.id.clone(),
            gate: self.gate.clone(),
            action: self.action.as_str().to_string(),
            epoch_apply: self.epoch_apply,
            state: match &self.state {
                IntentState::Pending => "pending".into(),
                IntentState::Applied { .. } => "applied".into(),
            },
            params_patch: self.params_patch.clone(),
            metrics: self.metrics.summary.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DecisionPayload {
    pub gate: String,
    pub action: GateAction,
    pub reason: String,
    pub intent_id: String,
    pub epoch: u64,
    pub metrics: IntentMetrics,
    pub params_patch: JsonValue,
}

impl DecisionPayload {
    pub fn to_json(&self) -> JsonValue {
        let mut root = JsonMap::new();
        root.insert("gate".into(), JsonValue::String(self.gate.clone()));
        root.insert(
            "action".into(),
            JsonValue::String(self.action.as_str().into()),
        );
        root.insert("reason".into(), JsonValue::String(self.reason.clone()));
        root.insert(
            "intent_id".into(),
            JsonValue::String(self.intent_id.clone()),
        );
        root.insert(
            "epoch".into(),
            JsonValue::Number(JsonNumber::from(self.epoch)),
        );
        root.insert("metrics_summary".into(), self.metrics.summary.clone());
        root.insert("metrics_raw".into(), self.metrics.raw.clone());
        root.insert("params_patch".into(), self.params_patch.clone());
        JsonValue::Object(root)
    }
}

#[derive(Clone, Debug)]
pub struct GateEval {
    pub enter_ok: bool,
    pub exit_ok: bool,
    pub reason: String,
    pub metrics: JsonValue,
}

impl Default for GateEval {
    fn default() -> Self {
        Self {
            enter_ok: false,
            exit_ok: true,
            reason: String::new(),
            metrics: JsonValue::Null,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct GateSnapshot {
    pub name: String,
    pub state: String,
    pub enter_streak: u64,
    pub exit_streak: u64,
    pub streak_required: u64,
    pub last_reason: String,
    pub last_metrics: JsonValue,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct GovernorStatus {
    pub enabled: bool,
    pub epoch: u64,
    pub window_secs: u64,
    pub gates: Vec<GateSnapshot>,
    pub pending: Vec<IntentSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct IntentSummary {
    pub id: String,
    pub gate: String,
    pub action: String,
    pub epoch_apply: u64,
    pub state: String,
    pub params_patch: JsonValue,
    pub metrics: JsonValue,
}

pub trait SignalProvider: Send + Sync {
    fn chain_sample(&self, window_secs: u64) -> ChainSample;
    fn dns_sample(&self, window_secs: u64) -> DnsSample;
}

#[derive(Clone, Default)]
pub struct ChainSample {
    pub block_spacing: Option<Vec<u64>>,
    pub difficulty: Option<Vec<u64>>,
    pub replay: Option<RatioSample>,
    pub peer_liveness: Option<RatioSample>,
    pub fee_band: Option<FeeBand>,
}

#[derive(Clone, Default)]
pub struct DnsSample {
    pub txt_success: Option<RatioSample>,
    pub dispute_share: Option<RatioSample>,
    pub completion: Option<RatioSample>,
    pub settle_durations_ms: Option<Vec<u64>>,
    pub stake_coverage_ratio: Option<f64>,
    pub settlement_p90_ct: Option<u64>,
}

#[derive(Clone, Copy, Default)]
pub struct RatioSample {
    pub numerator: u64,
    pub denominator: u64,
}

impl RatioSample {
    pub fn ratio(&self) -> Option<f64> {
        if self.denominator == 0 {
            None
        } else {
            Some(self.numerator as f64 / self.denominator as f64)
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct FeeBand {
    pub median: u64,
    pub p90: u64,
}

pub struct LiveSignalProvider {
    chain: Arc<Mutex<Blockchain>>,
}

impl LiveSignalProvider {
    pub fn new(chain: Arc<Mutex<Blockchain>>) -> Self {
        Self { chain }
    }
}

#[cfg(feature = "telemetry")]
fn fee_band_sample() -> Option<FeeBand> {
    Some(FeeBand {
        median: crate::fees::policy::consumer_p50(),
        p90: crate::fees::policy::consumer_p90(),
    })
}

#[cfg(not(feature = "telemetry"))]
fn fee_band_sample() -> Option<FeeBand> {
    None
}

impl SignalProvider for LiveSignalProvider {
    fn chain_sample(&self, window_secs: u64) -> ChainSample {
        let guard = self.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut block_spacing = Vec::new();
        let mut difficulty = Vec::new();
        let blocks = guard.chain.len();
        let window = window_secs.max(2) as usize;
        if blocks >= 2 {
            let start = blocks.saturating_sub(window.max(2));
            let slice = &guard.chain[start..];
            for pair in slice.windows(2) {
                let delta = pair[1]
                    .timestamp_millis
                    .saturating_sub(pair[0].timestamp_millis);
                block_spacing.push(delta);
            }
            for block in slice {
                difficulty.push(block.difficulty);
            }
        }
        ChainSample {
            block_spacing: if block_spacing.is_empty() {
                None
            } else {
                Some(block_spacing)
            },
            difficulty: if difficulty.is_empty() {
                None
            } else {
                Some(difficulty)
            },
            replay: replay_success_ratio(&guard, window),
            peer_liveness: peer_liveness_ratio(window_secs),
            fee_band: fee_band_sample(),
        }
    }

    fn dns_sample(&self, _window_secs: u64) -> DnsSample {
        let window = _window_secs.max(1);
        let snapshot = dns::governance_metrics_snapshot(window);
        let locked = dns::total_locked_stake();
        let txt = ratio_from_counts(snapshot.txt_successes, snapshot.txt_attempts);
        let completion_total = snapshot
            .auction_completions
            .saturating_add(snapshot.auction_cancels);
        let completion_ratio = ratio_from_counts(snapshot.auction_completions, completion_total);
        let dispute_total = completion_total.saturating_add(snapshot.stake_unlock_events);
        let dispute_ratio = ratio_from_counts(
            snapshot
                .auction_cancels
                .saturating_add(snapshot.stake_unlock_events),
            dispute_total,
        );
        let durations_ms = if snapshot.settle_durations_secs.is_empty() {
            None
        } else {
            Some(
                snapshot
                    .settle_durations_secs
                    .iter()
                    .map(|secs| secs.saturating_mul(1000))
                    .collect(),
            )
        };
        let settlement_p90 = percentile_u64(&snapshot.settlement_amounts_ct, 0.9);
        let coverage_ratio = settlement_p90.and_then(|p90| {
            if p90 == 0 {
                None
            } else {
                Some(locked as f64 / p90 as f64)
            }
        });
        DnsSample {
            txt_success: txt,
            dispute_share: dispute_ratio,
            completion: completion_ratio,
            settle_durations_ms: durations_ms,
            stake_coverage_ratio: coverage_ratio,
            settlement_p90_ct: settlement_p90,
        }
    }
}

fn peer_liveness_ratio(window_secs: u64) -> Option<RatioSample> {
    if let Some(store) = crate::peer_metrics_store::store() {
        let retention = window_secs.max(60);
        let map = store.load(retention);
        let mut successes = 0u64;
        let mut total = 0u64;
        for metrics in map.values() {
            successes = successes.saturating_add(metrics.requests);
            let drops: u64 = metrics.drops.values().copied().sum();
            total = total.saturating_add(metrics.requests.saturating_add(drops));
        }
        if total == 0 {
            None
        } else {
            Some(RatioSample {
                numerator: successes,
                denominator: total,
            })
        }
    } else {
        None
    }
}

struct GateRuntime {
    name: String,
    enter_streak: u64,
    exit_streak: u64,
    streak_required: u64,
    state: GateState,
    last_eval: GateEval,
}

#[derive(Clone, Copy, Debug)]
enum GateState {
    Inactive,
    Active,
    Rehearsal,
    Trade,
}

impl GateRuntime {
    fn new(name: &str, required: u64) -> Self {
        Self {
            name: name.to_string(),
            enter_streak: 0,
            exit_streak: 0,
            streak_required: required.max(1),
            state: GateState::Inactive,
            last_eval: GateEval::default(),
        }
    }

    fn snapshot(&self) -> GateSnapshot {
        GateSnapshot {
            name: self.name.clone(),
            state: match self.state {
                GateState::Inactive => "inactive".into(),
                GateState::Active => "active".into(),
                GateState::Rehearsal => "rehearsal".into(),
                GateState::Trade => "trade".into(),
            },
            enter_streak: self.enter_streak,
            exit_streak: self.exit_streak,
            streak_required: self.streak_required,
            last_reason: self.last_eval.reason.clone(),
            last_metrics: self.last_eval.metrics.clone(),
        }
    }
}

struct GovernorStore {
    inner: Arc<Mutex<SimpleDb>>,
}

impl GovernorStore {
    fn open(path: &Path) -> Self {
        let db = SimpleDb::open_named(names::GOVERNOR, path.to_str().unwrap());
        Self {
            inner: Arc::new(Mutex::new(db)),
        }
    }

    fn list_pending(&self) -> Vec<IntentRecord> {
        let db_guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let keys = db_guard.keys_with_prefix(INTENT_PREFIX);
        let mut pending = Vec::new();
        for key in keys {
            if let Some(bytes) = db_guard.get(&key) {
                if let Ok(intent) = json::from_slice::<IntentRecord>(&bytes) {
                    if matches!(intent.state, IntentState::Pending) {
                        pending.push(intent);
                    }
                }
            }
        }
        pending
    }

    fn save(&self, intent: &IntentRecord) {
        let bytes = json::to_vec(intent).expect("serialize intent");
        let key = format!("{INTENT_PREFIX}{}", intent.id);
        let mut db = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        db.insert(&key, bytes);
    }
}

pub struct GovernorHandle {
    shared: Arc<SharedState>,
    cancel: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl GovernorHandle {
    pub fn status(&self) -> GovernorStatus {
        self.shared.status()
    }

    pub fn decisions(&self, limit: usize) -> Vec<IntentSummary> {
        self.shared.decisions(limit)
    }

    pub fn snapshot(&self, epoch: u64) -> Option<JsonValue> {
        governor_snapshot::load_snapshot(&self.shared.base_path, epoch)
    }
}

impl Drop for GovernorHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.task.lock().unwrap().take() {
            handle.abort();
        }
    }
}

struct SharedState {
    enabled: bool,
    base_path: String,
    window_secs: u64,
    store: GovernorStore,
    epoch: AtomicU64,
    pending: Mutex<Vec<IntentRecord>>,
    log: Mutex<VecDeque<IntentRecord>>,
    gates: Mutex<HashMap<String, GateRuntime>>,
}

impl SharedState {
    fn new(config: &GovernorConfig, store: GovernorStore) -> Self {
        let mut gates = HashMap::new();
        let required = (crate::EPOCH_BLOCKS / config.window_secs.max(1)).max(1);
        gates.insert(
            "operational".into(),
            GateRuntime::new("operational", required),
        );
        gates.insert("naming".into(), GateRuntime::new("naming", required));
        Self {
            enabled: config.enabled,
            base_path: config.base_path.to_string_lossy().into_owned(),
            window_secs: config.window_secs,
            store,
            epoch: AtomicU64::new(0),
            pending: Mutex::new(Vec::new()),
            log: Mutex::new(VecDeque::with_capacity(LOG_LIMIT)),
            gates: Mutex::new(gates),
        }
    }

    fn status(&self) -> GovernorStatus {
        let gates = {
            let guard = self.gates.lock().unwrap_or_else(|e| e.into_inner());
            guard.values().map(|g| g.snapshot()).collect()
        };
        let pending = {
            let guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            guard.iter().map(|p| p.summary()).collect()
        };
        GovernorStatus {
            enabled: self.enabled,
            epoch: self.epoch.load(AtomicOrdering::Relaxed),
            window_secs: self.window_secs,
            gates,
            pending,
        }
    }

    fn set_epoch(&self, epoch: u64) {
        self.epoch.store(epoch, AtomicOrdering::Relaxed);
    }

    fn record(&self, intent: IntentRecord) {
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        log.push_back(intent.clone());
        if log.len() > LOG_LIMIT {
            log.pop_front();
        }
        self.store.save(&intent);
    }

    fn decisions(&self, limit: usize) -> Vec<IntentSummary> {
        let log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        log.iter()
            .rev()
            .take(limit)
            .map(|entry| entry.summary())
            .collect()
    }

    fn update_gate(
        &self,
        gate: &str,
        state: GateState,
        enter: u64,
        exit: u64,
        required: u64,
        eval: &GateEval,
    ) {
        let mut guard = self.gates.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entry(gate.to_string())
            .and_modify(|runtime| {
                runtime.state = state;
                runtime.enter_streak = enter;
                runtime.exit_streak = exit;
                runtime.streak_required = required;
                runtime.last_eval = eval.clone();
            })
            .or_insert_with(|| GateRuntime {
                name: gate.to_string(),
                enter_streak: enter,
                exit_streak: exit,
                streak_required: required,
                state,
                last_eval: eval.clone(),
            });
    }
}

pub fn spawn(chain: Arc<Mutex<Blockchain>>, config: GovernorConfig) -> Option<GovernorHandle> {
    if !config.enabled {
        return None;
    }
    let store = GovernorStore::open(&config.db_path);
    let shared = Arc::new(SharedState::new(&config, store));
    {
        let mut pending = shared.pending.lock().unwrap_or_else(|e| e.into_inner());
        let mut existing = shared.store.list_pending();
        existing.sort_by(|a, b| a.epoch_apply.cmp(&b.epoch_apply));
        *pending = existing;
    }
    let cancel = CancellationToken::new();
    let signal_provider: Arc<dyn SignalProvider> =
        Arc::new(LiveSignalProvider::new(Arc::clone(&chain)));
    let shared_task = Arc::clone(&shared);
    let cancel_task = cancel.clone();
    let handle = runtime::spawn(async move {
        run_service(
            chain,
            shared_task,
            signal_provider,
            cancel_task,
            config.window_secs,
        )
        .await;
    });
    Some(GovernorHandle {
        shared,
        cancel,
        task: Mutex::new(Some(handle)),
    })
}

async fn run_service(
    chain: Arc<Mutex<Blockchain>>,
    shared: Arc<SharedState>,
    provider: Arc<dyn SignalProvider>,
    cancel: CancellationToken,
    window_secs: u64,
) {
    let mut intent_seq = 0u64;
    let mut operational_ctrl = OperationalController::new(window_secs);
    let mut naming_ctrl = NamingController::new(window_secs);
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let epoch = {
            let guard = chain.lock().unwrap_or_else(|e| e.into_inner());
            guard.block_height / crate::EPOCH_BLOCKS
        };
        shared.set_epoch(epoch);
        let base_path = shared.base_path.clone();
        let chain_sample = provider.chain_sample(window_secs);
        if let Some(eval) = operational_ctrl.evaluate(epoch, &chain_sample) {
            if let Some(intent) = plan_intent(
                &base_path,
                &mut intent_seq,
                "operational",
                eval.action,
                epoch,
                eval.metrics.clone(),
                eval.reason.clone(),
            ) {
                process_intent(intent, &chain, &shared);
            }
        }
        shared.update_gate(
            "operational",
            operational_ctrl.gate_state(),
            operational_ctrl.enter(),
            operational_ctrl.exit(),
            operational_ctrl.required(),
            operational_ctrl.eval(),
        );
        let dns_sample = provider.dns_sample(window_secs);
        if let Some(eval) = naming_ctrl.evaluate(epoch, &dns_sample) {
            if let Some(intent) = plan_intent(
                &base_path,
                &mut intent_seq,
                "naming",
                eval.action,
                epoch,
                eval.metrics.clone(),
                eval.reason.clone(),
            ) {
                process_intent(intent, &chain, &shared);
            }
        }
        shared.update_gate(
            "naming",
            naming_ctrl.gate_state(),
            naming_ctrl.enter(),
            naming_ctrl.exit(),
            naming_ctrl.required(),
            naming_ctrl.eval(),
        );
        sleep(Duration::from_secs(window_secs.max(1))).await;
    }
}

struct PlannedEval {
    action: GateAction,
    reason: String,
    metrics: JsonValue,
}

fn plan_intent(
    base_path: &str,
    seq: &mut u64,
    gate: &str,
    action: GateAction,
    epoch: u64,
    summary: JsonValue,
    reason: String,
) -> Option<IntentRecord> {
    let id = format!("{gate}-{epoch}-{seq}");
    *seq = seq.saturating_add(1);
    let params_patch = match action {
        GateAction::Enter => json_map(vec![("operational", JsonValue::Bool(true))]),
        GateAction::Exit => json_map(vec![("operational", JsonValue::Bool(false))]),
        GateAction::Rehearsal => {
            json_map(vec![("naming_mode", JsonValue::String("rehearsal".into()))])
        }
        GateAction::Trade => json_map(vec![("naming_mode", JsonValue::String("trade".into()))]),
    };
    let epoch_apply = epoch + 1;
    let payload = DecisionPayload {
        gate: gate.into(),
        action,
        reason,
        intent_id: id.clone(),
        epoch: epoch_apply,
        metrics: IntentMetrics {
            summary: summary.clone(),
            raw: summary.clone(),
        },
        params_patch: params_patch.clone(),
    };
    let hash =
        governor_snapshot::persist_snapshot(base_path, &payload, epoch_apply).unwrap_or([0u8; 32]);
    let metrics_clone = summary.clone();
    Some(IntentRecord {
        id,
        gate: gate.into(),
        action,
        created_epoch: epoch,
        epoch_apply,
        params_patch,
        metrics: IntentMetrics {
            summary,
            raw: metrics_clone,
        },
        snapshot_hash_hex: hex::encode(hash),
        state: IntentState::Pending,
    })
}

fn process_intent(intent: IntentRecord, chain: &Arc<Mutex<Blockchain>>, shared: &Arc<SharedState>) {
    {
        let mut pending = shared.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.push(intent.clone());
    }
    shared.record(intent.clone());
    apply_intent(chain, shared, intent);
}

fn apply_intent(
    chain: &Arc<Mutex<Blockchain>>,
    shared: &Arc<SharedState>,
    mut intent: IntentRecord,
) {
    {
        let mut guard = chain.lock().unwrap_or_else(|e| e.into_inner());
        let params_snapshot = guard.params.clone();
        let mut runtime = Runtime::new(&mut *guard);
        match intent.action {
            GateAction::Enter => runtime.set_launch_operational(true),
            GateAction::Exit => runtime.set_launch_operational(false),
            GateAction::Rehearsal => runtime.set_dns_rehearsal(true),
            GateAction::Trade => runtime.set_dns_rehearsal(false),
        }
        runtime.set_current_params(&params_snapshot);
    }
    intent.state = IntentState::Applied {
        epoch: intent.epoch_apply,
    };
    shared.record(intent.clone());
    let mut pending = shared.pending.lock().unwrap_or_else(|e| e.into_inner());
    pending.retain(|p| p.id != intent.id);
    shared.store.save(&intent);
}

fn json_map(entries: Vec<(&str, JsonValue)>) -> JsonValue {
    let mut map = JsonMap::new();
    for (k, v) in entries {
        map.insert(k.into(), v);
    }
    JsonValue::Object(map)
}

fn replay_success_ratio(chain: &Blockchain, window_blocks: usize) -> Option<RatioSample> {
    if window_blocks < 2 {
        return None;
    }
    let len = chain.chain.len();
    if len < 2 {
        return None;
    }
    let start = len.saturating_sub(window_blocks);
    let slice = &chain.chain[start..];
    if slice.is_empty() {
        return None;
    }
    let mut success = 0u64;
    let mut total = 0u64;
    for block in slice {
        total += 1;
        if validate_and_apply(chain, block).is_ok() {
            success += 1;
        }
    }
    Some(RatioSample {
        numerator: success,
        denominator: total,
    })
}

fn ratio_from_counts(numerator: u64, denominator: u64) -> Option<RatioSample> {
    if denominator == 0 {
        None
    } else {
        Some(RatioSample {
            numerator: numerator.min(denominator),
            denominator,
        })
    }
}

fn percentile_u64(values: &[u64], pct: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() - 1) as f64 * pct.clamp(0.0, 1.0)).round() as usize;
    sorted.get(idx).copied()
}

fn percentile_f64(values: &VecDeque<f64>, pct: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = values.iter().copied().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let idx = ((sorted.len() - 1) as f64 * pct.clamp(0.0, 1.0)).round() as usize;
    sorted.get(idx).copied()
}

fn median_f64(values: &VecDeque<f64>) -> Option<f64> {
    percentile_f64(values, 0.5)
}

fn median_u64(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() / 2])
}

fn std_dev(values: &VecDeque<f64>) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let var = values
        .iter()
        .map(|v| {
            let diff = v - mean;
            diff * diff
        })
        .sum::<f64>()
        / (values.len() as f64);
    var.sqrt()
}

fn diff_percentile(values: &VecDeque<f64>, pct: f64) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mut diffs: Vec<f64> = values
        .iter()
        .zip(values.iter().skip(1))
        .map(|(prev, next)| (next - prev).abs())
        .collect();
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let idx = ((diffs.len() - 1) as f64 * pct.clamp(0.0, 1.0)).round() as usize;
    diffs.get(idx).copied().unwrap_or(0.0)
}

struct OperationalController {
    required_streak: u64,
    enter_streak: u64,
    exit_streak: u64,
    active: bool,
    last_eval: GateEval,
    history_capacity: usize,
    smooth_history: VecDeque<f64>,
    slope_history: VecDeque<f64>,
    replay_history: VecDeque<f64>,
    peer_history: VecDeque<f64>,
}

impl OperationalController {
    fn new(window_secs: u64) -> Self {
        let epoch_secs = crate::EPOCH_BLOCKS.max(1);
        let required_streak = (epoch_secs / window_secs.max(1)).max(1);
        let history_capacity = (required_streak as usize).max(4) * 2;
        Self {
            required_streak,
            enter_streak: 0,
            exit_streak: 0,
            active: false,
            last_eval: GateEval::default(),
            history_capacity,
            smooth_history: VecDeque::with_capacity(history_capacity),
            slope_history: VecDeque::with_capacity(history_capacity),
            replay_history: VecDeque::with_capacity(history_capacity),
            peer_history: VecDeque::with_capacity(history_capacity),
        }
    }

    fn evaluate(&mut self, epoch: u64, sample: &ChainSample) -> Option<PlannedEval> {
        let smooth = block_smoothness(sample.block_spacing.as_ref()?);
        let slope = difficulty_slope(sample.difficulty.as_ref()?);
        let peer = sample.peer_liveness.and_then(|r| r.ratio()).unwrap_or(0.0);
        let replay = sample.replay.and_then(|r| r.ratio()).unwrap_or(0.0);
        let history_capacity = self.history_capacity;
        Self::push_history(&mut self.smooth_history, history_capacity, smooth);
        Self::push_history(&mut self.slope_history, history_capacity, slope);
        Self::push_history(&mut self.replay_history, history_capacity, replay);
        Self::push_history(&mut self.peer_history, history_capacity, peer);
        if self.smooth_history.len() < 2
            || self.slope_history.len() < 2
            || self.replay_history.len() < 2
            || self.peer_history.len() < 2
        {
            return None;
        }
        // Get previous values from second-to-last position (before current push)
        let smooth_prev = *self.smooth_history.get(self.smooth_history.len() - 2).unwrap();
        let smooth_std = std_dev(&self.smooth_history);
        let smooth_enter_band = smooth_prev + smooth_std;
        let smooth_exit_band = smooth_prev + smooth_std * 2.0;

        let slope_prev = self.slope_history.get(self.slope_history.len() - 2).unwrap().abs();
        let slope_std = std_dev(&self.slope_history);
        let slope_enter_band = slope_prev.max(slope_std);
        let slope_exit_band = slope_prev + slope_std.max(0.0) * 2.0;

        let replay_prev = *self.replay_history.get(self.replay_history.len() - 2).unwrap();
        let replay_delta = diff_percentile(&self.replay_history, 0.75);

        let peer_median = median_f64(&self.peer_history).unwrap_or(peer);
        let peer_delta = diff_percentile(&self.peer_history, 0.75);

        let metrics = json_map(vec![
            (
                "block_smoothness_ratio",
                JsonValue::Number(JsonNumber::from_f64(smooth).unwrap_or(JsonNumber::from(0u64))),
            ),
            (
                "difficulty_slope",
                JsonValue::Number(JsonNumber::from_f64(slope).unwrap_or(JsonNumber::from(0u64))),
            ),
            (
                "replay_success_ratio",
                JsonValue::Number(JsonNumber::from_f64(replay).unwrap_or(JsonNumber::from(0u64))),
            ),
            (
                "peer_liveness",
                JsonValue::Number(JsonNumber::from_f64(peer).unwrap_or(JsonNumber::from(0u64))),
            ),
        ]);
        let smooth_enter_ok = smooth <= smooth_enter_band;
        let smooth_exit_ok = smooth <= smooth_exit_band;
        let slope_enter_ok = slope.abs() <= slope_enter_band;
        let slope_exit_ok = slope.abs() <= slope_exit_band;
        let replay_enter_ok = replay >= (replay_prev + replay_delta);
        let replay_exit_ok = replay >= (replay_prev - replay_delta);
        let peer_enter_ok = peer >= peer_median;
        let peer_exit_ok = peer + peer_delta >= peer_median;

        let enter_ok = smooth_enter_ok && slope_enter_ok && replay_enter_ok && peer_enter_ok;
        let exit_ok = smooth_exit_ok && slope_exit_ok && replay_exit_ok && peer_exit_ok;
        let reason = format!(
            "smooth={smooth:.4}/{smooth_enter_band:.4} slope={slope:.4} peer={peer:.4}/{peer_median:.4} replay={replay:.4}"
        );
        self.last_eval = GateEval {
            enter_ok,
            exit_ok,
            reason: reason.clone(),
            metrics: metrics.clone(),
        };
        if enter_ok {
            self.enter_streak = self.enter_streak.saturating_add(1);
        } else {
            self.enter_streak = 0;
        }
        if !exit_ok {
            self.exit_streak = self.exit_streak.saturating_add(1);
        } else {
            self.exit_streak = 0;
        }
        if !self.active && self.enter_streak >= self.required_streak {
            self.active = true;
            self.exit_streak = 0;
            Some(PlannedEval {
                action: GateAction::Enter,
                reason: format!("epoch {epoch}: operational enter streak met"),
                metrics,
            })
        } else if self.active && self.exit_streak >= self.required_streak {
            self.active = false;
            self.enter_streak = 0;
            Some(PlannedEval {
                action: GateAction::Exit,
                reason: format!("epoch {epoch}: operational exit streak met"),
                metrics,
            })
        } else {
            None
        }
    }

    fn push_history(history: &mut VecDeque<f64>, capacity: usize, value: f64) {
        if history.len() == capacity {
            history.pop_front();
        }
        history.push_back(value);
    }

    fn gate_state(&self) -> GateState {
        if self.active {
            GateState::Active
        } else {
            GateState::Inactive
        }
    }

    fn enter(&self) -> u64 {
        self.enter_streak
    }

    fn exit(&self) -> u64 {
        self.exit_streak
    }

    fn required(&self) -> u64 {
        self.required_streak
    }

    fn eval(&self) -> &GateEval {
        &self.last_eval
    }
}

fn block_smoothness(spacing: &[u64]) -> f64 {
    if spacing.is_empty() {
        return 0.0;
    }
    let mut values: Vec<f64> = spacing.iter().map(|v| *v as f64).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let median = values[values.len() / 2];
    if median == 0.0 {
        return 0.0;
    }
    let mut deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mad = deviations[deviations.len() / 2];
    mad / median.max(1.0)
}

fn difficulty_slope(series: &[u64]) -> f64 {
    if series.len() < 2 {
        return 0.0;
    }
    let xs: Vec<f64> = (0..series.len()).map(|i| i as f64).collect();
    let ys: Vec<f64> = series.iter().map(|v| *v as f64).collect();
    linear_slope(&xs, &ys)
}

fn linear_slope(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 2 {
        return 0.0;
    }
    let mean_x = xs.iter().take(n).sum::<f64>() / n as f64;
    let mean_y = ys.iter().take(n).sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dx = xs[i] - mean_x;
        let dy = ys[i] - mean_y;
        num += dx * dy;
        den += dx * dx;
    }
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

struct NamingController {
    required_streak: u64,
    rehearsal_ready: bool,
    trade_active: bool,
    rehearsal_streak: u64,
    trade_enter_streak: u64,
    trade_exit_streak: u64,
    history_capacity: usize,
    txt_history: VecDeque<f64>,
    dispute_history: VecDeque<f64>,
    completion_history: VecDeque<f64>,
    coverage_history: VecDeque<f64>,
    settlement_history: VecDeque<f64>,
    duration_history: VecDeque<f64>,
    last_eval: GateEval,
}

impl NamingController {
    fn new(window_secs: u64) -> Self {
        let epoch_secs = crate::EPOCH_BLOCKS.max(1);
        let required = (epoch_secs / window_secs.max(1)).max(1);
        let history_capacity = (required as usize).max(4) * 2;
        Self {
            required_streak: required,
            rehearsal_ready: false,
            trade_active: false,
            rehearsal_streak: 0,
            trade_enter_streak: 0,
            trade_exit_streak: 0,
            history_capacity,
            txt_history: VecDeque::with_capacity(history_capacity),
            dispute_history: VecDeque::with_capacity(history_capacity),
            completion_history: VecDeque::with_capacity(history_capacity),
            coverage_history: VecDeque::with_capacity(history_capacity),
            settlement_history: VecDeque::with_capacity(history_capacity),
            duration_history: VecDeque::with_capacity(history_capacity),
            last_eval: GateEval::default(),
        }
    }

    fn evaluate(&mut self, epoch: u64, sample: &DnsSample) -> Option<PlannedEval> {
        let txt_ratio = sample.txt_success.as_ref()?.ratio()?;
        let dispute_ratio = sample.dispute_share.as_ref()?.ratio()?;
        let completion_ratio = sample.completion.as_ref()?.ratio()?;
        let coverage = sample.stake_coverage_ratio.unwrap_or(0.0);
        let settle_p90 = sample.settlement_p90_ct.unwrap_or(0) as f64;
        let duration_median = sample
            .settle_durations_ms
            .as_ref()
            .and_then(|vals| median_u64(vals).map(|ms| ms as f64 / 1000.0))
            .unwrap_or(0.0);

        let history_capacity = self.history_capacity;
        Self::push_history(&mut self.txt_history, history_capacity, txt_ratio);
        Self::push_history(&mut self.dispute_history, history_capacity, dispute_ratio);
        Self::push_history(
            &mut self.completion_history,
            history_capacity,
            completion_ratio,
        );
        Self::push_history(&mut self.coverage_history, history_capacity, coverage);
        Self::push_history(&mut self.settlement_history, history_capacity, settle_p90);
        Self::push_history(
            &mut self.duration_history,
            history_capacity,
            duration_median,
        );

        if self.txt_history.len() < 2
            || self.dispute_history.len() < 2
            || self.completion_history.len() < 2
            || self.settlement_history.len() < 2
        {
            return None;
        }

        // Get previous values from second-to-last position (before current push)
        let txt_prev = *self.txt_history.get(self.txt_history.len() - 2).unwrap();
        let dispute_prev = *self.dispute_history.get(self.dispute_history.len() - 2).unwrap();
        let completion_prev = *self.completion_history.get(self.completion_history.len() - 2).unwrap();
        let settlement_prev = *self.settlement_history.get(self.settlement_history.len() - 2).unwrap();
        let duration_prev = *self.duration_history.get(self.duration_history.len() - 2).unwrap();

        let txt_delta = diff_percentile(&self.txt_history, 0.75);
        let dispute_delta = diff_percentile(&self.dispute_history, 0.75);
        let duration_delta = diff_percentile(&self.duration_history, 0.75);

        let rehearsal_ok = txt_ratio >= (txt_prev + txt_delta)
            && dispute_ratio + dispute_delta <= dispute_prev
            && completion_ratio >= completion_prev;
        let duration_ok = duration_median <= duration_prev + duration_delta;
        let trade_ready = coverage >= settlement_prev && duration_ok;
        let dispute_exit = dispute_ratio >= dispute_prev + dispute_delta;
        let txt_exit = txt_ratio + txt_delta <= txt_prev;

        let metrics = json_map(vec![
            (
                "txt_success_ratio",
                JsonValue::Number(
                    JsonNumber::from_f64(txt_ratio).unwrap_or(JsonNumber::from(0u64)),
                ),
            ),
            (
                "dispute_ratio",
                JsonValue::Number(
                    JsonNumber::from_f64(dispute_ratio).unwrap_or(JsonNumber::from(0u64)),
                ),
            ),
            (
                "completion_ratio",
                JsonValue::Number(
                    JsonNumber::from_f64(completion_ratio).unwrap_or(JsonNumber::from(0u64)),
                ),
            ),
            (
                "stake_coverage_ratio",
                JsonValue::Number(JsonNumber::from_f64(coverage).unwrap_or(JsonNumber::from(0u64))),
            ),
            (
                "settlement_p90_ct",
                JsonValue::Number(JsonNumber::from(settle_p90 as u64)),
            ),
        ]);
        let reason = format!(
            "txt={txt_ratio:.2}/{txt_prev:.2} dispute={dispute_ratio:.2}/{dispute_prev:.2} completion={completion_ratio:.2}/{completion_prev:.2} coverage={coverage:.2}/{settlement_prev:.2}"
        );
        self.last_eval = GateEval {
            enter_ok: rehearsal_ok,
            exit_ok: !(dispute_exit || txt_exit),
            reason: reason.clone(),
            metrics: metrics.clone(),
        };
        if rehearsal_ok {
            self.rehearsal_streak = self.rehearsal_streak.saturating_add(1);
        } else {
            self.rehearsal_streak = 0;
        }
        if trade_ready {
            self.trade_enter_streak = self.trade_enter_streak.saturating_add(1);
        } else {
            self.trade_enter_streak = 0;
        }
        if dispute_exit || txt_exit {
            self.trade_exit_streak = self.trade_exit_streak.saturating_add(1);
        } else {
            self.trade_exit_streak = 0;
        }

        if !self.rehearsal_ready && self.rehearsal_streak >= self.required_streak {
            self.rehearsal_ready = true;
            self.trade_exit_streak = 0;
            return Some(PlannedEval {
                action: GateAction::Rehearsal,
                reason: format!("epoch {epoch}: naming rehearsal window met"),
                metrics,
            });
        }

        if self.rehearsal_ready
            && !self.trade_active
            && self.trade_enter_streak >= self.required_streak
        {
            self.trade_active = true;
            self.trade_exit_streak = 0;
            return Some(PlannedEval {
                action: GateAction::Trade,
                reason: format!("epoch {epoch}: naming trade window met"),
                metrics,
            });
        }

        if self.trade_active && self.trade_exit_streak >= self.required_streak {
            self.trade_active = false;
            self.trade_enter_streak = 0;
            return Some(PlannedEval {
                action: GateAction::Rehearsal,
                reason: format!("epoch {epoch}: naming trade exit window met"),
                metrics,
            });
        }

        None
    }

    fn push_history(history: &mut VecDeque<f64>, capacity: usize, value: f64) {
        if history.len() == capacity {
            history.pop_front();
        }
        history.push_back(value);
    }

    fn gate_state(&self) -> GateState {
        if self.trade_active {
            GateState::Trade
        } else if self.rehearsal_ready {
            GateState::Rehearsal
        } else {
            GateState::Inactive
        }
    }

    fn enter(&self) -> u64 {
        if self.trade_active {
            self.trade_enter_streak
        } else {
            self.rehearsal_streak
        }
    }

    fn exit(&self) -> u64 {
        self.trade_exit_streak
    }

    fn required(&self) -> u64 {
        self.required_streak
    }

    fn eval(&self) -> &GateEval {
        &self.last_eval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)] // Test utility struct for future test expansion
    struct MockProvider {
        chain_samples: VecDeque<ChainSample>,
        dns_samples: VecDeque<DnsSample>,
    }

    #[allow(dead_code)] // Test utility methods for future test expansion
    impl MockProvider {
        fn new() -> Self {
            Self {
                chain_samples: VecDeque::new(),
                dns_samples: VecDeque::new(),
            }
        }

        fn push_chain(&mut self, sample: ChainSample) {
            self.chain_samples.push_back(sample);
        }

        fn push_dns(&mut self, sample: DnsSample) {
            self.dns_samples.push_back(sample);
        }
    }

    impl SignalProvider for MockProvider {
        fn chain_sample(&self, _window_secs: u64) -> ChainSample {
            self.chain_samples.front().cloned().unwrap_or_default()
        }

        fn dns_sample(&self, _window_secs: u64) -> DnsSample {
            self.dns_samples.front().cloned().unwrap_or_default()
        }
    }

    #[test]
    fn block_smoothness_zero_when_empty() {
        assert_eq!(block_smoothness(&[]), 0.0);
    }

    #[test]
    fn operational_controller_enters_with_streak() {
        let mut controller = OperationalController::new(crate::EPOCH_BLOCKS * 2);
        let baseline = ChainSample {
            block_spacing: Some(vec![500, 1500, 500, 1500]),
            difficulty: Some(vec![100, 100, 100, 100]),
            replay: Some(RatioSample {
                numerator: 4,
                denominator: 5,
            }),
            peer_liveness: Some(RatioSample {
                numerator: 7,
                denominator: 10,
            }),
            fee_band: None,
        };
        assert!(controller.evaluate(1, &baseline).is_none());
        let improved = ChainSample {
            block_spacing: Some(vec![1000, 1000, 1000, 1000]),
            difficulty: Some(vec![100, 100, 100, 100]),
            replay: Some(RatioSample {
                numerator: 5,
                denominator: 5,
            }),
            peer_liveness: Some(RatioSample {
                numerator: 9,
                denominator: 10,
            }),
            fee_band: None,
        };
        let eval = controller.evaluate(2, &improved).expect("enter");
        assert_eq!(eval.action, GateAction::Enter);
    }

    #[test]
    fn naming_controller_transitions_to_trade() {
        let mut ctrl = NamingController::new(crate::EPOCH_BLOCKS * 2);
        let baseline = DnsSample {
            txt_success: Some(RatioSample {
                numerator: 6,
                denominator: 10,
            }),
            dispute_share: Some(RatioSample {
                numerator: 3,
                denominator: 10,
            }),
            completion: Some(RatioSample {
                numerator: 5,
                denominator: 10,
            }),
            settle_durations_ms: Some(vec![2200, 2400]),
            stake_coverage_ratio: Some(80.0),
            settlement_p90_ct: Some(100),
        };
        assert!(ctrl.evaluate(1, &baseline).is_none());
        let rehearsal_sample = DnsSample {
            txt_success: Some(RatioSample {
                numerator: 9,
                denominator: 10,
            }),
            dispute_share: Some(RatioSample {
                numerator: 1,
                denominator: 10,
            }),
            completion: Some(RatioSample {
                numerator: 9,
                denominator: 10,
            }),
            settle_durations_ms: Some(vec![1500, 1400]),
            stake_coverage_ratio: Some(150.0),
            settlement_p90_ct: Some(100),
        };
        let eval = ctrl.evaluate(2, &rehearsal_sample).expect("rehearsal");
        assert_eq!(eval.action, GateAction::Rehearsal);
        let trade_sample = DnsSample {
            txt_success: rehearsal_sample.txt_success.clone(),
            dispute_share: rehearsal_sample.dispute_share.clone(),
            completion: rehearsal_sample.completion.clone(),
            settle_durations_ms: Some(vec![1200, 1100]),
            stake_coverage_ratio: Some(400.0),
            settlement_p90_ct: Some(80),
        };
        let trade_eval = ctrl.evaluate(3, &trade_sample).expect("trade");
        assert_eq!(trade_eval.action, GateAction::Trade);
    }
}
