use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::telemetry;

/// Hardware capability descriptor for a provider or workload.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    /// Number of CPU cores available.
    pub cpu_cores: u8,
    /// Optional GPU model identifier.
    #[serde(default)]
    pub gpu: Option<String>,
    /// GPU memory in megabytes.
    #[serde(default)]
    pub gpu_memory_mb: u32,
    /// Optional accelerator identifier (e.g. TPU model).
    #[serde(default)]
    pub accelerator: Option<String>,
}

#[derive(Serialize)]
pub struct SchedulerStats {
    pub success: u64,
    pub capability_mismatch: u64,
    pub reputation_failure: u64,
    pub active_jobs: u64,
    pub utilization: HashMap<String, u64>,
}

impl Capability {
    /// Return true if `self` satisfies the required `other` capability.
    pub fn matches(&self, other: &Capability) -> bool {
        if self.cpu_cores < other.cpu_cores {
            return false;
        }
        if let Some(ref req_gpu) = other.gpu {
            if self.gpu.as_deref() != Some(req_gpu.as_str()) {
                return false;
            }
        }
        if other.gpu_memory_mb > 0 && self.gpu_memory_mb < other.gpu_memory_mb {
            return false;
        }
        if let Some(ref req_acc) = other.accelerator {
            if self.accelerator.as_deref() != Some(req_acc.as_str()) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug)]
struct OfferEntry {
    capability: Capability,
    reputation: i64,
}

const RECENT_WINDOW: usize = 100;

#[derive(Clone, Copy)]
enum MatchOutcome {
    Success,
    CapabilityMismatch,
    ReputationFailure,
}

struct SchedulerState {
    offers: HashMap<String, OfferEntry>,
    utilization: HashMap<String, u64>,
    reputation: HashMap<String, i64>,
    recent: VecDeque<MatchOutcome>,
    active_jobs: u64,
}

impl SchedulerState {
    fn register_offer(&mut self, provider: &str, capability: Capability, reputation: i64) {
        self.offers.insert(
            provider.to_owned(),
            OfferEntry {
                capability,
                reputation,
            },
        );
    }

    fn match_job(&mut self, need: &Capability) -> Option<String> {
        let mut best: Option<(String, i64)> = None;
        let mut mem_insufficient = false;
        for (prov, entry) in &self.offers {
            if let Some(req_gpu) = &need.gpu {
                if entry.capability.gpu.as_deref() == Some(req_gpu.as_str())
                    && entry.capability.gpu_memory_mb < need.gpu_memory_mb
                {
                    mem_insufficient = true;
                }
            }
            if entry.capability.matches(need) {
                let rep = *self.reputation.get(prov).unwrap_or(&entry.reputation);
                if rep >= 0 {
                    match best {
                        Some((_, best_rep)) if rep <= best_rep => {}
                        _ => best = Some((prov.clone(), rep)),
                    }
                }
            }
        }
        let mut outcome = MatchOutcome::CapabilityMismatch;
        if let Some((prov, _)) = &best {
            let cap_label = if let Some(g) = &self.offers[prov].capability.gpu {
                g.clone()
            } else if let Some(a) = &self.offers[prov].capability.accelerator {
                a.clone()
            } else {
                "cpu".to_string()
            };
            self.utilization
                .entry(cap_label)
                .and_modify(|v| *v += 1)
                .or_insert(1);
            self.active_jobs += 1;
            outcome = MatchOutcome::Success;
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                {
                    telemetry::SCHEDULER_MATCH_TOTAL
                        .with_label_values(&["success"])
                        .inc();
                    telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
                }
            }
        } else if mem_insufficient {
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_MATCH_TOTAL
                    .with_label_values(&["capability_mismatch"])
                    .inc();
            }
        } else {
            let any_cap_match = self.offers.values().any(|e| e.capability.matches(need));
            if any_cap_match {
                outcome = MatchOutcome::ReputationFailure;
                if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                    #[cfg(feature = "telemetry")]
                    telemetry::SCHEDULER_MATCH_TOTAL
                        .with_label_values(&["reputation_failure"])
                        .inc();
                }
            } else if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_MATCH_TOTAL
                    .with_label_values(&["capability_mismatch"])
                    .inc();
            }
        }
        self.recent.push_back(outcome);
        if self.recent.len() > RECENT_WINDOW {
            self.recent.pop_front();
        }
        best.map(|(p, _)| p)
    }

    fn record_success(&mut self, provider: &str) {
        let rep = self.reputation.entry(provider.to_owned()).or_insert(0);
        *rep += 1;
        {
            let mut store = REPUTATION_STORE.lock().unwrap();
            store.adjust(provider, 1);
        }
        if self.active_jobs > 0 {
            self.active_jobs -= 1;
        }
        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
            #[cfg(feature = "telemetry")]
            {
                telemetry::REPUTATION_ADJUST_TOTAL
                    .with_label_values(&["success"])
                    .inc();
                telemetry::PROVIDER_REPUTATION_SCORE
                    .with_label_values(&[provider])
                    .set(*rep);
                telemetry::SCHEDULER_REPUTATION_SCORE.observe(*rep as f64);
                telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
            }
        }
    }

    fn record_failure(&mut self, provider: &str) {
        let rep = self.reputation.entry(provider.to_owned()).or_insert(0);
        *rep -= 1;
        {
            let mut store = REPUTATION_STORE.lock().unwrap();
            store.adjust(provider, -1);
        }
        if self.active_jobs > 0 {
            self.active_jobs -= 1;
        }
        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
            #[cfg(feature = "telemetry")]
            {
                telemetry::REPUTATION_ADJUST_TOTAL
                    .with_label_values(&["failure"])
                    .inc();
                telemetry::PROVIDER_REPUTATION_SCORE
                    .with_label_values(&[provider])
                    .set(*rep);
                telemetry::SCHEDULER_REPUTATION_SCORE.observe(*rep as f64);
                telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
            }
        }
    }

    fn metrics(&self) -> serde_json::Value {
        serde_json::json!({
            "reputation": self.reputation,
            "utilization": self.utilization,
        })
    }

    fn stats(&self) -> SchedulerStats {
        let mut success = 0;
        let mut capability_mismatch = 0;
        let mut reputation_failure = 0;
        for r in &self.recent {
            match r {
                MatchOutcome::Success => success += 1,
                MatchOutcome::CapabilityMismatch => capability_mismatch += 1,
                MatchOutcome::ReputationFailure => reputation_failure += 1,
            }
        }
        SchedulerStats {
            success,
            capability_mismatch,
            reputation_failure,
            active_jobs: self.active_jobs,
            utilization: self.utilization.clone(),
        }
    }
}

static SCHEDULER: Lazy<Mutex<SchedulerState>> = Lazy::new(|| {
    let rep = {
        let mut store = REPUTATION_STORE.lock().unwrap();
        store.decay(provider_reputation_decay(), provider_reputation_retention());
        store
            .data
            .iter()
            .map(|(k, v)| (k.clone(), v.score))
            .collect()
    };
    Mutex::new(SchedulerState {
        offers: HashMap::new(),
        utilization: HashMap::new(),
        reputation: rep,
        recent: VecDeque::new(),
        active_jobs: 0,
    })
});

#[derive(Serialize, Deserialize, Default)]
struct ReputationEntry {
    score: i64,
    last_update: u64,
}

#[derive(Default)]
pub struct ReputationStore {
    path: PathBuf,
    data: HashMap<String, ReputationEntry>,
}

impl ReputationStore {
    pub fn load(path: PathBuf) -> Self {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(data) = serde_json::from_slice(&bytes) {
                return Self { path, data };
            }
        }
        Self {
            path,
            data: HashMap::new(),
        }
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_vec(&self.data) {
            if let Some(parent) = self.path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&self.path, json);
        }
    }

    pub fn adjust(&mut self, provider: &str, delta: i64) {
        let now = current_ts();
        let entry = self
            .data
            .entry(provider.to_string())
            .or_insert(ReputationEntry {
                score: 0,
                last_update: now,
            });
        entry.score += delta;
        entry.last_update = now;
        self.save();
    }

    pub fn get(&self, provider: &str) -> i64 {
        self.data.get(provider).map(|e| e.score).unwrap_or(0)
    }

    fn decay(&mut self, rate: f64, retention: u64) {
        let now = current_ts();
        self.data.retain(|_, e| {
            let elapsed = now.saturating_sub(e.last_update);
            if elapsed > 0 {
                let decayed = (e.score as f64) * (1.0 - rate).powf(elapsed as f64);
                e.score = decayed.round() as i64;
                e.last_update = now;
            }
            !(e.score == 0 && elapsed > retention)
        });
        self.save();
    }
}

static REPUTATION_STORE: Lazy<Mutex<ReputationStore>> = Lazy::new(|| {
    let path = reputation_db_path();
    Mutex::new(ReputationStore::load(path))
});

static PROVIDER_REPUTATION_DECAY: AtomicU64 = AtomicU64::new(f64::to_bits(0.05));
static PROVIDER_REPUTATION_RETENTION: AtomicU64 = AtomicU64::new(7 * 24 * 60 * 60);
static SCHEDULER_METRICS_ENABLED: AtomicBool = AtomicBool::new(true);

fn provider_reputation_decay() -> f64 {
    f64::from_bits(PROVIDER_REPUTATION_DECAY.load(Ordering::Relaxed))
}

pub fn set_provider_reputation_decay(rate: f64) {
    PROVIDER_REPUTATION_DECAY.store(rate.to_bits(), Ordering::Relaxed);
}

fn provider_reputation_retention() -> u64 {
    PROVIDER_REPUTATION_RETENTION.load(Ordering::Relaxed)
}

pub fn set_provider_reputation_retention(secs: u64) {
    PROVIDER_REPUTATION_RETENTION.store(secs, Ordering::Relaxed);
}

pub fn set_scheduler_metrics_enabled(val: bool) {
    SCHEDULER_METRICS_ENABLED.store(val, Ordering::Relaxed);
}

fn reputation_db_path() -> PathBuf {
    std::env::var("TB_REPUTATION_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("reputation.json")
        })
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

pub fn register_offer(provider: &str, capability: Capability, reputation: i64) {
    SCHEDULER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .register_offer(provider, capability, reputation);
}

pub fn match_offer(need: &Capability) -> Option<String> {
    let start = std::time::Instant::now();
    let res = SCHEDULER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .match_job(need);
    if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
        #[cfg(feature = "telemetry")]
        telemetry::SCHEDULER_MATCH_LATENCY_SECONDS.observe(start.elapsed().as_secs_f64());
    }
    res
}

pub fn record_success(provider: &str) {
    SCHEDULER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .record_success(provider);
}

pub fn record_failure(provider: &str) {
    SCHEDULER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .record_failure(provider);
}

pub fn metrics() -> serde_json::Value {
    SCHEDULER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .metrics()
}

pub fn stats() -> SchedulerStats {
    SCHEDULER.lock().unwrap_or_else(|e| e.into_inner()).stats()
}

pub fn reputation_get(provider: &str) -> i64 {
    REPUTATION_STORE.lock().unwrap().get(provider)
}

pub fn reset_for_test() {
    {
        let mut s = SCHEDULER.lock().unwrap();
        s.offers.clear();
        s.utilization.clear();
        s.reputation.clear();
        s.recent.clear();
        s.active_jobs = 0;
        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
            #[cfg(feature = "telemetry")]
            telemetry::SCHEDULER_ACTIVE_JOBS.set(0);
        }
    }
    REPUTATION_STORE.lock().unwrap().data.clear();
}
