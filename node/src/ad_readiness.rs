#![forbid(unsafe_code)]

use crate::simple_db::{names, SimpleDb};
use crate::util::binary_struct::{ensure_exhausted, DecodeError};
use concurrency::Lazy;
use foundation_serialization::binary_cursor::{Reader as BinaryReader, Writer as BinaryWriter};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_WINDOW_SECS: u64 = 24 * 60 * 60;
const DEFAULT_WINDOW_SECS: u64 = 6 * 60 * 60;
const DEFAULT_MIN_VIEWERS: u64 = 250;
const DEFAULT_MIN_HOSTS: u64 = 25;
const DEFAULT_MIN_PROVIDERS: u64 = 10;
const KEY_EVENTS: &str = "events";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AdReadinessConfig {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
}

impl Default for AdReadinessConfig {
    fn default() -> Self {
        Self {
            window_secs: DEFAULT_WINDOW_SECS,
            min_unique_viewers: DEFAULT_MIN_VIEWERS,
            min_host_count: DEFAULT_MIN_HOSTS,
            min_provider_count: DEFAULT_MIN_PROVIDERS,
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

    pub fn decision(&self) -> ReadinessDecision {
        ReadinessDecision {
            snapshot: self.inner.snapshot(),
        }
    }

    pub fn snapshot(&self) -> AdReadinessSnapshot {
        self.inner.snapshot()
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
        let state = AdReadinessState::from_events(initial_events);
        Self {
            config: RwLock::new(config),
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
            .unwrap_or_else(|poison| poison.into_inner()) = config;
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

#[derive(Default)]
struct AdReadinessState {
    events: VecDeque<ReadinessEvent>,
    viewer_counts: HashMap<[u8; 32], u64>,
    host_counts: HashMap<String, u64>,
    provider_counts: HashMap<String, u64>,
    last_updated: u64,
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

    fn prune(&mut self, now: u64, window_secs: u64) {
        let cutoff = now.saturating_sub(window_secs.max(1));
        while let Some(front) = self.events.front() {
            if front.ts >= cutoff {
                break;
            }
            let front = self.events.pop_front().expect("front element");
            self.decrement(front);
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
            return AdReadinessSnapshot {
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
            };
        }
        let now = current_timestamp();
        self.prune(now, config.window_secs);
        let unique_viewers = self.viewer_counts.len() as u64;
        let host_count = self.host_counts.len() as u64;
        let provider_count = self.provider_counts.len() as u64;
        let mut blockers = Vec::new();
        if unique_viewers < config.min_unique_viewers {
            blockers.push("insufficient_unique_viewers".to_string());
        }
        if host_count < config.min_host_count {
            blockers.push("insufficient_host_diversity".to_string());
        }
        if provider_count < config.min_provider_count {
            blockers.push("insufficient_provider_diversity".to_string());
        }
        let ready = blockers.is_empty();
        AdReadinessSnapshot {
            window_secs: config.window_secs,
            min_unique_viewers: config.min_unique_viewers,
            min_host_count: config.min_host_count,
            min_provider_count: config.min_provider_count,
            unique_viewers,
            host_count,
            provider_count,
            ready,
            blockers,
            last_updated: self.last_updated,
        }
    }
}

#[derive(Clone)]
struct ReadinessEvent {
    ts: u64,
    viewer: [u8; 32],
    host: String,
    provider: Option<String>,
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
