#![forbid(unsafe_code)]

use crate::simple_db::{names, SimpleDb};
use crate::util::binary_struct::{ensure_exhausted, DecodeError};
use ad_market::CohortPriceSnapshot;
use concurrency::Lazy;
use crypto_suite::hashing::blake3;
use foundation_serialization::binary_cursor::{Reader as BinaryReader, Writer as BinaryWriter};
use foundation_serialization::{Deserialize, Serialize};
use std::cmp;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zkp::{ReadinessPrivacyProof, ReadinessStatement, ReadinessWitness};

const MAX_WINDOW_SECS: u64 = 24 * 60 * 60;
const DEFAULT_WINDOW_SECS: u64 = 6 * 60 * 60;
const DEFAULT_MIN_VIEWERS: u64 = 250;
const DEFAULT_MIN_HOSTS: u64 = 25;
const DEFAULT_MIN_PROVIDERS: u64 = 10;
const KEY_EVENTS: &str = "events";
const KEY_CONFIG: &str = "config";

fn new_privacy_seed() -> [u8; 32] {
    let now = current_timestamp();
    let pid = std::process::id();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&now.to_le_bytes());
    hasher.update(&pid.to_le_bytes());
    hasher.finalize().into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessConfig {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    #[serde(default)]
    pub use_percentile_thresholds: bool,
    #[serde(default)]
    pub viewer_percentile: u8,
    #[serde(default)]
    pub host_percentile: u8,
    #[serde(default)]
    pub provider_percentile: u8,
    #[serde(default)]
    pub ema_smoothing_ppm: u32,
    #[serde(default)]
    pub floor_unique_viewers: u64,
    #[serde(default)]
    pub floor_host_count: u64,
    #[serde(default)]
    pub floor_provider_count: u64,
    #[serde(default)]
    pub cap_unique_viewers: u64,
    #[serde(default)]
    pub cap_host_count: u64,
    #[serde(default)]
    pub cap_provider_count: u64,
    #[serde(default)]
    pub percentile_buckets: u16,
}

impl Default for AdReadinessConfig {
    fn default() -> Self {
        Self {
            window_secs: DEFAULT_WINDOW_SECS,
            min_unique_viewers: DEFAULT_MIN_VIEWERS,
            min_host_count: DEFAULT_MIN_HOSTS,
            min_provider_count: DEFAULT_MIN_PROVIDERS,
            use_percentile_thresholds: false,
            viewer_percentile: 90,
            host_percentile: 75,
            provider_percentile: 50,
            ema_smoothing_ppm: 200_000,
            floor_unique_viewers: 0,
            floor_host_count: 0,
            floor_provider_count: 0,
            cap_unique_viewers: 0,
            cap_host_count: 0,
            cap_provider_count: 0,
            percentile_buckets: 12,
        }
    }
}

#[derive(Clone)]
pub(crate) struct AdReadinessPersistence {
    db: Arc<Mutex<SimpleDb>>,
}

impl AdReadinessPersistence {
    fn open(path: &str) -> Self {
        let db = SimpleDb::open_named(names::GATEWAY_AD_READINESS, path);
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    fn load(&self, window_secs: u64) -> VecDeque<ReadinessEvent> {
        let cutoff = current_timestamp().saturating_sub(window_secs.max(1));
        let mut guard = self.db.lock().unwrap_or_else(|poison| poison.into_inner());
        let events = guard
            .get(KEY_EVENTS)
            .and_then(|bytes| decode_events(&bytes).ok())
            .unwrap_or_default();
        let filtered: VecDeque<ReadinessEvent> = events
            .into_iter()
            .filter(|event| event.ts >= cutoff)
            .collect();
        let bytes = encode_events(&filtered);
        guard.insert(KEY_EVENTS, bytes);
        filtered
    }

    fn persist(&self, events: &VecDeque<ReadinessEvent>) {
        let bytes = encode_events(events);
        let mut guard = self.db.lock().unwrap_or_else(|poison| poison.into_inner());
        guard.insert(KEY_EVENTS, bytes);
    }

    fn persist_config(&self, cfg: &AdReadinessConfig) {
        if let Ok(bytes) = foundation_serialization::json::to_vec(cfg) {
            let mut guard = self.db.lock().unwrap_or_else(|poison| poison.into_inner());
            guard.insert(KEY_CONFIG, bytes);
        }
    }

    fn load_config(&self) -> Option<AdReadinessConfig> {
        let guard = self.db.lock().unwrap_or_else(|poison| poison.into_inner());
        guard.get(KEY_CONFIG).and_then(|bytes| {
            foundation_serialization::json::from_slice::<AdReadinessConfig>(&bytes).ok()
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessSnapshot {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    pub unique_viewers: u64,
    pub host_count: u64,
    pub provider_count: u64,
    pub ready: bool,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub blockers: Vec<String>,
    pub last_updated: u64,
    #[serde(default)]
    pub zk_proof: Option<ReadinessPrivacyProof>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub total_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub settlement_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub ct_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub it_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub market_ct_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub market_it_price_usd_micros: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub cohort_utilization: Vec<AdReadinessCohortUtilization>,
    #[serde(default)]
    pub utilization_summary: Option<AdReadinessUtilizationSummary>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub ready_streak_windows: u64,
    #[serde(default)]
    pub segment_readiness: Option<AdSegmentReadiness>,
}

impl Default for AdReadinessSnapshot {
    fn default() -> Self {
        Self {
            window_secs: DEFAULT_WINDOW_SECS,
            min_unique_viewers: DEFAULT_MIN_VIEWERS,
            min_host_count: DEFAULT_MIN_HOSTS,
            min_provider_count: DEFAULT_MIN_PROVIDERS,
            unique_viewers: 0,
            host_count: 0,
            provider_count: 0,
            ready: false,
            blockers: Vec::new(),
            last_updated: 0,
            zk_proof: None,
            total_usd_micros: 0,
            settlement_count: 0,
            ct_price_usd_micros: 0,
            it_price_usd_micros: 0,
            market_ct_price_usd_micros: 0,
            market_it_price_usd_micros: 0,
            cohort_utilization: Vec::new(),
            utilization_summary: None,
            ready_streak_windows: 0,
            segment_readiness: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessCohortUtilization {
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub badges: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub interest_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_bucket_id: Option<String>,
    pub price_per_mib_usd_micros: u64,
    pub target_utilization_ppm: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub observed_utilization_ppm: u32,
    pub delta_ppm: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessUtilizationSummary {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub cohort_count: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub mean_ppm: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub min_ppm: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub max_ppm: u32,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub last_updated: u64,
}

/// Per-segment readiness stats for domain tiers, interest tags, and presence buckets.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdSegmentReadiness {
    /// Domain tier readiness: tier -> {supply_ppm, readiness_score}
    #[serde(default = "foundation_serialization::defaults::default")]
    pub domain_tiers: std::collections::HashMap<String, SegmentReadinessStats>,
    /// Interest tag readiness: tag -> {supply_ppm, readiness_score}
    #[serde(default = "foundation_serialization::defaults::default")]
    pub interest_tags: std::collections::HashMap<String, SegmentReadinessStats>,
    /// Presence bucket readiness: bucket_id -> {freshness_histogram, ready_slots}
    #[serde(default = "foundation_serialization::defaults::default")]
    pub presence_buckets: std::collections::HashMap<String, PresenceBucketReadiness>,
    /// Privacy budget status
    #[serde(default)]
    pub privacy_budget: Option<PrivacyBudgetStatus>,
}

/// Stats for a single segment (domain tier or interest tag).
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct SegmentReadinessStats {
    /// Supply in parts per million of total cohorts
    pub supply_ppm: u32,
    /// Readiness score (0-100)
    pub readiness_score: u8,
    /// Count of cohorts in this segment
    pub cohort_count: u64,
}

/// Readiness stats for a presence bucket.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PresenceBucketReadiness {
    /// Freshness histogram: <1h, 1-6h, 6-24h, >24h (in ppm)
    #[serde(default = "foundation_serialization::defaults::default")]
    pub freshness_histogram: FreshnessHistogramPpm,
    /// Number of ready impression slots
    pub ready_slots: u64,
    /// Source kind: "localnet" or "range_boost"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Freshness histogram in parts per million.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct FreshnessHistogramPpm {
    pub under_1h_ppm: u32,
    pub hours_1_to_6_ppm: u32,
    pub hours_6_to_24_ppm: u32,
    pub over_24h_ppm: u32,
}

/// Privacy budget status for readiness reporting.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct PrivacyBudgetStatus {
    /// Remaining budget in ppm
    pub remaining_ppm: u32,
    /// Number of requests denied due to privacy budget
    pub denied_count: u64,
    /// Last denial reason if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_denial_reason: Option<String>,
}

impl AdReadinessSnapshot {
    pub fn to_statement(&self) -> ReadinessStatement {
        ReadinessStatement {
            window_secs: self.window_secs,
            min_unique_viewers: self.min_unique_viewers,
            min_host_count: self.min_host_count,
            min_provider_count: self.min_provider_count,
            unique_viewers: self.unique_viewers,
            host_count: self.host_count,
            provider_count: self.provider_count,
            ready: self.ready,
            last_updated: self.last_updated,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReadinessDecision {
    pub snapshot: AdReadinessSnapshot,
}

impl ReadinessDecision {
    pub fn ready(&self) -> bool {
        self.snapshot.ready
    }

    pub fn blockers(&self) -> &[String] {
        &self.snapshot.blockers
    }
}

#[derive(Clone)]
pub struct AdReadinessHandle {
    inner: Arc<AdReadinessInner>,
}

impl AdReadinessHandle {
    pub fn new(config: AdReadinessConfig) -> Self {
        Self::with_persistence(config, None, VecDeque::new())
    }

    pub fn open_with_storage(path: &str, mut config: AdReadinessConfig) -> Self {
        AdReadinessInner::normalize_config(&mut config);
        let persistence = AdReadinessPersistence::open(path);
        let initial_events = persistence.load(config.window_secs);
        Self::with_persistence(config, Some(persistence), initial_events)
    }

    fn with_persistence(
        config: AdReadinessConfig,
        persistence: Option<AdReadinessPersistence>,
        initial_events: VecDeque<ReadinessEvent>,
    ) -> Self {
        Self {
            inner: Arc::new(AdReadinessInner::with_state(
                config,
                persistence,
                initial_events,
            )),
        }
    }

    pub fn update_config(&self, config: AdReadinessConfig) {
        self.inner.update_config(config);
    }

    pub fn record_ack(&self, ts: u64, viewer: [u8; 32], host: &str, provider: Option<&str>) {
        self.inner.record_ack(ts, viewer, host, provider);
    }

    pub fn record_settlement(
        &self,
        ts: u64,
        usd_micros: u64,
        ct_price_usd_micros: u64,
        it_price_usd_micros: u64,
    ) {
        self.inner
            .record_settlement(ts, usd_micros, ct_price_usd_micros, it_price_usd_micros);
    }

    pub fn record_utilization(
        &self,
        cohorts: &[CohortPriceSnapshot],
        market_ct_price_usd_micros: u64,
        market_it_price_usd_micros: u64,
    ) {
        self.inner.record_utilization(
            cohorts,
            market_ct_price_usd_micros,
            market_it_price_usd_micros,
        );
    }

    pub fn decision(&self) -> ReadinessDecision {
        ReadinessDecision {
            snapshot: self.inner.snapshot(),
        }
    }

    pub fn snapshot(&self) -> AdReadinessSnapshot {
        self.inner.snapshot()
    }

    pub fn config(&self) -> AdReadinessConfig {
        self.inner.config()
    }
}

struct AdReadinessInner {
    config: RwLock<AdReadinessConfig>,
    state: Mutex<AdReadinessState>,
    persistence: Option<AdReadinessPersistence>,
}

impl AdReadinessInner {
    fn with_state(
        mut config: AdReadinessConfig,
        persistence: Option<AdReadinessPersistence>,
        initial_events: VecDeque<ReadinessEvent>,
    ) -> Self {
        Self::normalize_config(&mut config);
        let mut state = AdReadinessState::from_events(initial_events);
        // If config is persisted, prefer it to keep threshold math stable across restarts.
        let effective = if let Some(ref store) = persistence {
            store.load_config().unwrap_or_else(|| config.clone())
        } else {
            config.clone()
        };
        state.apply_config(&effective);
        Self {
            config: RwLock::new(effective),
            state: Mutex::new(state),
            persistence,
        }
    }

    pub(crate) fn normalize_config(config: &mut AdReadinessConfig) {
        if config.window_secs == 0 {
            config.window_secs = DEFAULT_WINDOW_SECS;
        }
        if config.window_secs > MAX_WINDOW_SECS {
            config.window_secs = MAX_WINDOW_SECS;
        }
    }

    fn update_config(&self, mut config: AdReadinessConfig) {
        Self::normalize_config(&mut config);
        *self
            .config
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = config.clone();
        if let Some(store) = &self.persistence {
            store.persist_config(&config);
        }
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .apply_config(&config);
    }

    fn config(&self) -> AdReadinessConfig {
        self.config
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    fn record_ack(&self, ts: u64, viewer: [u8; 32], host: &str, provider: Option<&str>) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let config = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        guard.push(ts, viewer, host, provider, &config);
        let snapshot = guard.events.clone();
        drop(guard);
        self.persist_events(&snapshot);
    }

    fn record_settlement(
        &self,
        ts: u64,
        usd_micros: u64,
        ct_price_usd_micros: u64,
        it_price_usd_micros: u64,
    ) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let config = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        guard.record_settlement(
            ts,
            usd_micros,
            ct_price_usd_micros,
            it_price_usd_micros,
            &config,
        );
    }

    fn record_utilization(
        &self,
        cohorts: &[CohortPriceSnapshot],
        market_ct_price_usd_micros: u64,
        market_it_price_usd_micros: u64,
    ) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.record_utilization(
            cohorts,
            market_ct_price_usd_micros,
            market_it_price_usd_micros,
        );
    }

    fn snapshot(&self) -> AdReadinessSnapshot {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let config = self
            .config
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let snapshot = guard.snapshot(&config);
        let events = guard.events.clone();
        drop(guard);
        self.persist_events(&events);
        snapshot
    }

    fn persist_events(&self, events: &VecDeque<ReadinessEvent>) {
        if let Some(persistence) = &self.persistence {
            persistence.persist(events);
        }
    }
}

struct AdReadinessState {
    events: VecDeque<ReadinessEvent>,
    viewer_counts: HashMap<[u8; 32], u64>,
    host_counts: HashMap<String, u64>,
    provider_counts: HashMap<String, u64>,
    last_updated: u64,
    privacy_seed: [u8; 32],
    settlements: VecDeque<SettlementObservation>,
    last_ct_price_usd_micros: u64,
    last_it_price_usd_micros: u64,
    market_ct_price_usd_micros: u64,
    market_it_price_usd_micros: u64,
    cohort_utilization: Vec<AdReadinessCohortUtilization>,
    utilization_summary: Option<AdReadinessUtilizationSummary>,
    last_utilization_update: u64,
    // EMA smoothed dynamic thresholds
    ema_min_unique_viewers: u64,
    ema_min_host_count: u64,
    ema_min_provider_count: u64,
    // Rehearsal streak tracking
    last_window_id: u64,
    ready_streak_windows: u64,
}

impl Default for AdReadinessState {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            viewer_counts: HashMap::new(),
            host_counts: HashMap::new(),
            provider_counts: HashMap::new(),
            last_updated: 0,
            privacy_seed: new_privacy_seed(),
            settlements: VecDeque::new(),
            last_ct_price_usd_micros: 0,
            last_it_price_usd_micros: 0,
            market_ct_price_usd_micros: 0,
            market_it_price_usd_micros: 0,
            cohort_utilization: Vec::new(),
            utilization_summary: None,
            last_utilization_update: 0,
            ema_min_unique_viewers: 0,
            ema_min_host_count: 0,
            ema_min_provider_count: 0,
            last_window_id: 0,
            ready_streak_windows: 0,
        }
    }
}

impl AdReadinessState {
    fn from_events(events: VecDeque<ReadinessEvent>) -> Self {
        let mut state = Self::default();
        for event in events {
            state.ingest(event);
        }
        state
    }

    fn ingest(&mut self, event: ReadinessEvent) {
        *self.viewer_counts.entry(event.viewer).or_insert(0) += 1;
        *self.host_counts.entry(event.host.clone()).or_insert(0) += 1;
        if let Some(ref provider) = event.provider {
            if !provider.is_empty() {
                *self.provider_counts.entry(provider.clone()).or_insert(0) += 1;
            }
        }
        self.last_updated = event.ts;
        self.events.push_back(event);
    }

    fn apply_config(&mut self, _config: &AdReadinessConfig) {
        // No-op placeholder: thresholds are recomputed on demand; EMA carries over.
    }

    fn readiness_witness(&self) -> ReadinessWitness {
        ReadinessWitness::new(self.privacy_seed)
    }

    fn push(
        &mut self,
        ts: u64,
        viewer: [u8; 32],
        host: &str,
        provider: Option<&str>,
        config: &AdReadinessConfig,
    ) {
        self.prune(ts, config.window_secs);
        let event = ReadinessEvent {
            ts,
            viewer,
            host: host.to_string(),
            provider: provider.map(|p| p.to_string()),
        };
        self.ingest(event);
    }

    fn record_settlement(
        &mut self,
        ts: u64,
        usd_micros: u64,
        ct_price_usd_micros: u64,
        it_price_usd_micros: u64,
        config: &AdReadinessConfig,
    ) {
        self.prune(ts, config.window_secs);
        self.last_ct_price_usd_micros = ct_price_usd_micros;
        self.last_it_price_usd_micros = it_price_usd_micros;
        self.settlements.push_back(SettlementObservation {
            ts,
            usd_micros,
            ct_price_usd_micros,
            it_price_usd_micros,
        });
    }

    fn record_utilization(
        &mut self,
        cohorts: &[CohortPriceSnapshot],
        market_ct_price_usd_micros: u64,
        market_it_price_usd_micros: u64,
    ) {
        let now = current_timestamp();
        self.market_ct_price_usd_micros = market_ct_price_usd_micros;
        self.market_it_price_usd_micros = market_it_price_usd_micros;
        self.last_utilization_update = now;
        if cohorts.is_empty() {
            self.cohort_utilization.clear();
            self.utilization_summary = Some(AdReadinessUtilizationSummary {
                cohort_count: 0,
                mean_ppm: 0,
                min_ppm: 0,
                max_ppm: 0,
                last_updated: now,
            });
            return;
        }
        let mut entries = Vec::with_capacity(cohorts.len());
        let mut sum: u128 = 0;
        let mut min = u32::MAX;
        let mut max = 0u32;
        for cohort in cohorts {
            let observed = cohort.observed_utilization_ppm;
            let target = cohort.target_utilization_ppm;
            let delta = i64::from(observed) - i64::from(target);
            sum = sum.saturating_add(u128::from(observed));
            min = cmp::min(min, observed);
            max = cmp::max(max, observed);
            entries.push(AdReadinessCohortUtilization {
                domain: cohort.domain.clone(),
                provider: cohort.provider.clone(),
                badges: cohort.badges.clone(),
                price_per_mib_usd_micros: cohort.price_per_mib_usd_micros,
                target_utilization_ppm: target,
                observed_utilization_ppm: observed,
                delta_ppm: delta,
                domain_tier: None,
                interest_tags: Vec::new(),
                presence_bucket_id: None,
            });
        }
        let count = cohorts.len() as u64;
        let mean = if count == 0 {
            0
        } else {
            (sum / u128::from(count)) as u64
        };
        self.cohort_utilization = entries;
        self.utilization_summary = Some(AdReadinessUtilizationSummary {
            cohort_count: count,
            mean_ppm: mean,
            min_ppm: if min == u32::MAX { 0 } else { min },
            max_ppm: max,
            last_updated: now,
        });
    }

    fn prune(&mut self, now: u64, window_secs: u64) {
        let cutoff = now.saturating_sub(window_secs.max(1));
        while let Some(front) = self.events.front() {
            if front.ts >= cutoff {
                break;
            }
            let front = self.events.pop_front().expect("front element");
            self.decrement(front);
        }
        while let Some(front) = self.settlements.front() {
            if front.ts >= cutoff {
                break;
            }
            self.settlements.pop_front();
        }
        self.compact_maps();
    }

    fn decrement(&mut self, event: ReadinessEvent) {
        if let Some(entry) = self.viewer_counts.get_mut(&event.viewer) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                self.viewer_counts.remove(&event.viewer);
            }
        }
        if let Some(entry) = self.host_counts.get_mut(&event.host) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                self.host_counts.remove(&event.host);
            }
        }
        if let Some(provider) = event.provider {
            if provider.is_empty() {
                return;
            }
            if let Some(entry) = self.provider_counts.get_mut(&provider) {
                *entry = entry.saturating_sub(1);
                if *entry == 0 {
                    self.provider_counts.remove(&provider);
                }
            }
        }
    }

    fn compact_maps(&mut self) {
        self.viewer_counts.retain(|_, v| *v > 0);
        self.host_counts.retain(|_, v| *v > 0);
        self.provider_counts.retain(|_, v| *v > 0);
    }

    fn snapshot(&mut self, config: &AdReadinessConfig) -> AdReadinessSnapshot {
        if self.last_updated == 0 {
            let mut blockers = Vec::new();
            if config.min_unique_viewers > 0 {
                blockers.push("insufficient_unique_viewers".to_string());
            }
            if config.min_host_count > 0 {
                blockers.push("insufficient_host_diversity".to_string());
            }
            if config.min_provider_count > 0 {
                blockers.push("insufficient_provider_diversity".to_string());
            }
            let ready = blockers.is_empty();
            let mut snapshot = AdReadinessSnapshot {
                window_secs: config.window_secs,
                min_unique_viewers: config.min_unique_viewers,
                min_host_count: config.min_host_count,
                min_provider_count: config.min_provider_count,
                unique_viewers: 0,
                host_count: 0,
                provider_count: 0,
                ready,
                blockers,
                last_updated: 0,
                zk_proof: None,
                total_usd_micros: 0,
                settlement_count: 0,
                ct_price_usd_micros: self.last_ct_price_usd_micros,
                it_price_usd_micros: self.last_it_price_usd_micros,
                market_ct_price_usd_micros: self.market_ct_price_usd_micros,
                market_it_price_usd_micros: self.market_it_price_usd_micros,
                cohort_utilization: self.cohort_utilization.clone(),
                utilization_summary: self.utilization_summary.clone(),
                ready_streak_windows: 0,
                segment_readiness: None,
            };
            let statement = snapshot.to_statement();
            let proof = zkp::readiness::prove(&statement, &self.readiness_witness());
            snapshot.zk_proof = Some(proof);
            return snapshot;
        }
        let now = current_timestamp();
        self.prune(now, config.window_secs);
        let unique_viewers = self.viewer_counts.len() as u64;
        let host_count = self.host_counts.len() as u64;
        let provider_count = self.provider_counts.len() as u64;

        // Decide thresholds (static or dynamic with EMA and floors/caps)
        let (min_unique_viewers, min_host_count, min_provider_count) =
            if config.use_percentile_thresholds {
                let (pv, ph, pp) = self.compute_percentile_thresholds(now, config);
                let ev = self.apply_ema(self.ema_min_unique_viewers, pv, config.ema_smoothing_ppm);
                let eh = self.apply_ema(self.ema_min_host_count, ph, config.ema_smoothing_ppm);
                let ep = self.apply_ema(self.ema_min_provider_count, pp, config.ema_smoothing_ppm);
                self.ema_min_unique_viewers = ev;
                self.ema_min_host_count = eh;
                self.ema_min_provider_count = ep;
                (ev, eh, ep)
            } else {
                (
                    config.min_unique_viewers,
                    config.min_host_count,
                    config.min_provider_count,
                )
            };
        let mut blockers = Vec::new();
        if unique_viewers < min_unique_viewers {
            blockers.push("insufficient_unique_viewers".to_string());
        }
        if host_count < min_host_count {
            blockers.push("insufficient_host_diversity".to_string());
        }
        if provider_count < min_provider_count {
            blockers.push("insufficient_provider_diversity".to_string());
        }
        let ready = blockers.is_empty();
        let total_usd_micros: u64 = self.settlements.iter().map(|obs| obs.usd_micros).sum();
        let settlement_count = self.settlements.len() as u64;
        let ct_price = self
            .settlements
            .back()
            .map(|obs| obs.ct_price_usd_micros)
            .unwrap_or(self.last_ct_price_usd_micros);
        let it_price = self
            .settlements
            .back()
            .map(|obs| obs.it_price_usd_micros)
            .unwrap_or(self.last_it_price_usd_micros);

        let mut snapshot = AdReadinessSnapshot {
            window_secs: config.window_secs,
            min_unique_viewers,
            min_host_count,
            min_provider_count,
            unique_viewers,
            host_count,
            provider_count,
            ready,
            blockers,
            last_updated: self.last_updated,
            zk_proof: None,
            total_usd_micros,
            settlement_count,
            ct_price_usd_micros: ct_price,
            it_price_usd_micros: it_price,
            market_ct_price_usd_micros: self.market_ct_price_usd_micros,
            market_it_price_usd_micros: self.market_it_price_usd_micros,
            cohort_utilization: self.cohort_utilization.clone(),
            utilization_summary: self.utilization_summary.clone(),
            ready_streak_windows: self.ready_streak_windows,
            segment_readiness: None,
        };
        // Update rehearsal-ready streak based on window boundaries and readiness
        if snapshot.window_secs > 0 && snapshot.last_updated > 0 {
            let window_id = snapshot.last_updated / snapshot.window_secs;
            if window_id != self.last_window_id {
                self.last_window_id = window_id;
                if ready {
                    self.ready_streak_windows = self.ready_streak_windows.saturating_add(1);
                } else {
                    self.ready_streak_windows = 0;
                }
            } else if !ready {
                self.ready_streak_windows = 0;
            }
            snapshot.ready_streak_windows = self.ready_streak_windows;
        }
        let statement = snapshot.to_statement();
        let proof = zkp::readiness::prove(&statement, &self.readiness_witness());
        snapshot.zk_proof = Some(proof);
        snapshot
    }

    fn apply_ema(&self, prev: u64, sample: u64, smoothing_ppm: u32) -> u64 {
        let alpha = (smoothing_ppm as f64 / 1_000_000f64).clamp(0.0, 1.0);
        if prev == 0 {
            sample
        } else {
            (((1.0 - alpha) * (prev as f64)) + (alpha * (sample as f64))).round() as u64
        }
    }

    fn compute_percentile_thresholds(&self, now: u64, cfg: &AdReadinessConfig) -> (u64, u64, u64) {
        let window = cfg.window_secs.max(1);
        let buckets = cfg.percentile_buckets.max(4) as usize;
        let bucket_secs = (window / cfg.percentile_buckets.max(4) as u64).max(60);
        let cutoff = now.saturating_sub(window);
        let mut v_sets: Vec<std::collections::HashSet<[u8; 32]>> =
            vec![Default::default(); buckets];
        let mut h_sets: Vec<std::collections::HashSet<String>> = vec![Default::default(); buckets];
        let mut p_sets: Vec<std::collections::HashSet<String>> = vec![Default::default(); buckets];
        for ev in self.events.iter() {
            if ev.ts < cutoff {
                continue;
            }
            let offset = ev.ts.saturating_sub(cutoff);
            let idx = ((offset / bucket_secs) as usize).min(buckets.saturating_sub(1));
            v_sets[idx].insert(ev.viewer);
            h_sets[idx].insert(ev.host.clone());
            if let Some(ref pr) = ev.provider {
                if !pr.is_empty() {
                    p_sets[idx].insert(pr.clone());
                }
            }
        }
        let mut v: Vec<u64> = v_sets.iter().map(|s| s.len() as u64).collect();
        let mut h: Vec<u64> = h_sets.iter().map(|s| s.len() as u64).collect();
        let mut p: Vec<u64> = p_sets.iter().map(|s| s.len() as u64).collect();
        v.sort_unstable();
        h.sort_unstable();
        p.sort_unstable();
        let pv = percentile_of_sorted(&v, cfg.viewer_percentile);
        let ph = percentile_of_sorted(&h, cfg.host_percentile);
        let pp = percentile_of_sorted(&p, cfg.provider_percentile);
        (
            apply_floor_cap(pv, cfg.floor_unique_viewers, cfg.cap_unique_viewers),
            apply_floor_cap(ph, cfg.floor_host_count, cfg.cap_host_count),
            apply_floor_cap(pp, cfg.floor_provider_count, cfg.cap_provider_count),
        )
    }
}

fn apply_floor_cap(sample: u64, floor: u64, cap: u64) -> u64 {
    let floored = sample.max(floor);
    if cap == 0 {
        floored
    } else {
        floored.min(cap)
    }
}

fn percentile_of_sorted(data: &[u64], percentile: u8) -> u64 {
    if data.is_empty() {
        return 0;
    }
    let p = percentile.clamp(0, 100) as f64 / 100.0;
    let idx = ((data.len() - 1) as f64 * p).round() as usize;
    data[idx]
}

#[derive(Clone)]
struct ReadinessEvent {
    ts: u64,
    viewer: [u8; 32],
    host: String,
    provider: Option<String>,
}

#[derive(Clone)]
struct SettlementObservation {
    ts: u64,
    usd_micros: u64,
    ct_price_usd_micros: u64,
    it_price_usd_micros: u64,
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn encode_events(events: &VecDeque<ReadinessEvent>) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u64(events.len() as u64);
    for event in events {
        writer.write_u64(event.ts);
        writer.write_bytes(&event.viewer);
        writer.write_string(&event.host);
        match &event.provider {
            Some(provider) => {
                writer.write_bool(true);
                writer.write_string(provider);
            }
            None => {
                writer.write_bool(false);
            }
        }
    }
    writer.finish()
}

fn decode_events(bytes: &[u8]) -> Result<VecDeque<ReadinessEvent>, DecodeError> {
    let mut reader = BinaryReader::new(bytes);
    let len = reader.read_u64()? as usize;
    let mut events = VecDeque::with_capacity(len);
    for _ in 0..len {
        let ts = reader.read_u64()?;
        let mut viewer = [0u8; 32];
        let raw_viewer = reader.read_bytes()?;
        if raw_viewer.len() != 32 {
            return Err(DecodeError::InvalidFieldValue {
                field: "viewer",
                reason: format!("expected 32 bytes got {}", raw_viewer.len()),
            });
        }
        viewer.copy_from_slice(&raw_viewer);
        let host = reader.read_string()?;
        let has_provider = reader.read_bool()?;
        let provider = if has_provider {
            Some(reader.read_string()?)
        } else {
            None
        };
        events.push_back(ReadinessEvent {
            ts,
            viewer,
            host,
            provider,
        });
    }
    ensure_exhausted(&reader)?;
    Ok(events)
}

static GLOBAL_HANDLE: Lazy<RwLock<Option<AdReadinessHandle>>> = Lazy::new(|| RwLock::new(None));

pub fn install_global(handle: AdReadinessHandle) {
    *GLOBAL_HANDLE
        .write()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(handle);
}

pub fn global_snapshot() -> Option<AdReadinessSnapshot> {
    GLOBAL_HANDLE
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().map(|handle| handle.snapshot()))
}

pub fn record_settlement(
    ts: u64,
    usd_micros: u64,
    ct_price_usd_micros: u64,
    it_price_usd_micros: u64,
) {
    if let Ok(guard) = GLOBAL_HANDLE.read() {
        if let Some(handle) = guard.as_ref() {
            handle.record_settlement(ts, usd_micros, ct_price_usd_micros, it_price_usd_micros);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

    #[test]
    fn readiness_blocks_until_thresholds_met() {
        let handle = AdReadinessHandle::new(AdReadinessConfig {
            window_secs: 30,
            min_unique_viewers: 2,
            min_host_count: 1,
            min_provider_count: 1,
            ..AdReadinessConfig::default()
        });
        let base = current_timestamp();
        let viewer_one = [1u8; 32];
        let viewer_two = [2u8; 32];
        handle.record_ack(base, viewer_one, "example.test", Some("provider-a"));
        let decision = handle.decision();
        assert!(!decision.ready());
        assert!(decision
            .blockers()
            .contains(&"insufficient_unique_viewers".to_string()));
        handle.record_ack(base + 1, viewer_two, "example.test", Some("provider-a"));
        let decision = handle.decision();
        assert!(decision.ready());
    }

    #[test]
    fn readiness_expires_old_events() {
        let handle = AdReadinessHandle::new(AdReadinessConfig {
            window_secs: 5,
            min_unique_viewers: 1,
            min_host_count: 1,
            min_provider_count: 1,
            ..AdReadinessConfig::default()
        });
        let base = current_timestamp();
        let viewer = [7u8; 32];
        handle.record_ack(base, viewer, "host", Some("provider"));
        assert!(handle.decision().ready());
        handle.record_ack(base + 12, viewer, "host", Some("provider"));
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.unique_viewers, 1);
        assert!(snapshot.ready);
        assert_eq!(snapshot.blockers.len(), 0);
    }

    #[test]
    fn persistence_replays_ready_state() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ad_readiness");
        let cfg = AdReadinessConfig {
            window_secs: 30,
            min_unique_viewers: 1,
            min_host_count: 1,
            min_provider_count: 1,
            ..AdReadinessConfig::default()
        };
        let handle = AdReadinessHandle::open_with_storage(path.to_str().unwrap(), cfg.clone());
        let now = current_timestamp();
        handle.record_ack(now, [3u8; 32], "example.test", Some("provider-a"));
        assert!(handle.decision().ready());
        drop(handle);

        let replayed = AdReadinessHandle::open_with_storage(path.to_str().unwrap(), cfg);
        let snapshot = replayed.snapshot();
        assert!(snapshot.ready);
        assert_eq!(snapshot.unique_viewers, 1);
        assert_eq!(snapshot.host_count, 1);
        assert_eq!(snapshot.provider_count, 1);
        assert_eq!(snapshot.blockers.len(), 0);
    }
}
