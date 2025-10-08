use concurrency::{mutex, Lazy, MutexExt, MutexGuard, MutexT};
use foundation_serialization::json::{self, json, Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering as CmpOrdering;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use sys::paths;

use super::{courier, Accelerator};
#[cfg(feature = "telemetry")]
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
    /// Optional specialised accelerator type.
    #[serde(default)]
    pub accelerator: Option<Accelerator>,
    /// Accelerator memory in megabytes.
    #[serde(default)]
    pub accelerator_memory_mb: u32,
    /// Supported compute frameworks (e.g., CUDA, OpenCL).
    #[serde(default)]
    pub frameworks: Vec<String>,
}

#[derive(Serialize)]
pub struct SchedulerStats {
    pub success: u64,
    pub capability_mismatch: u64,
    pub reputation_failure: u64,
    pub preemptions: u64,
    pub active_jobs: u64,
    pub utilization: HashMap<String, u64>,
    pub effective_price: Option<u64>,
    pub queued_high: u64,
    pub queued_normal: u64,
    pub queued_low: u64,
    pub priority_miss: u64,
    pub pending: Vec<PendingJob>,
}

#[derive(Serialize)]
pub struct PendingJob {
    pub job_id: String,
    pub priority: Priority,
    pub effective_priority: f64,
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
            if self.accelerator.as_ref() != Some(req_acc) {
                return false;
            }
        }
        if other.accelerator_memory_mb > 0
            && self.accelerator_memory_mb < other.accelerator_memory_mb
        {
            return false;
        }
        for fw in &other.frameworks {
            if !self.frameworks.iter().any(|f| f == fw) {
                return false;
            }
        }
        true
    }

    /// Validate consistency of capability attributes.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.gpu.is_some() && self.gpu_memory_mb == 0 {
            return Err("missing gpu_memory_mb");
        }
        if self.accelerator.is_some() && self.accelerator_memory_mb == 0 {
            return Err("missing accelerator_memory_mb");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct OfferEntry {
    capability: Capability,
    reputation: i64,
    price_per_unit: u64,
    multiplier: f64,
}

const RECENT_WINDOW: usize = 100;
const ACCELERATOR_PREMIUM: f64 = 1.2;
const PRIORITY_MISS_SECS: u64 = 5;

static ADAPTIVE_THREADS: AtomicUsize = AtomicUsize::new(4);

fn adjust_thread_pool(active_jobs: u64) {
    let desired = active_jobs.clamp(1, 32) as usize;
    let prev = ADAPTIVE_THREADS.swap(desired, Ordering::Relaxed);
    if prev != desired {
        #[cfg(feature = "telemetry")]
        crate::telemetry::SCHEDULER_THREAD_COUNT.set(desired as i64);
    }
}

#[derive(Clone, Copy)]
enum MatchOutcome {
    Success,
    CapabilityMismatch,
    ReputationFailure,
}

#[derive(Clone, Copy, Debug)]
pub enum CancelReason {
    Client,
    Provider,
    Preempted,
    ClientTimeout,
    ProviderFault,
}

impl CancelReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            CancelReason::Client => "client",
            CancelReason::Provider => "provider",
            CancelReason::Preempted => "preempted",
            CancelReason::ClientTimeout => "client_timeout",
            CancelReason::ProviderFault => "provider_fault",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "provider" => CancelReason::Provider,
            "preempted" => CancelReason::Preempted,
            "client_timeout" => CancelReason::ClientTimeout,
            "provider_fault" => CancelReason::ProviderFault,
            _ => CancelReason::Client,
        }
    }
}

struct SchedulerState {
    offers: HashMap<String, OfferEntry>,
    utilization: HashMap<String, u64>,
    reputation: HashMap<String, i64>,
    recent: VecDeque<MatchOutcome>,
    active_jobs: u64,
    active: HashMap<String, ActiveAssignment>,
    preempt_total: u64,
    last_effective_price: Option<u64>,
    pending: BinaryHeap<QueuedJob>,
    active_by_rep: BinaryHeap<Reverse<ActiveRepEntry>>,
    active_low: u64,
    priority_miss_total: u64,
}

#[derive(Clone)]
struct ActiveAssignment {
    provider: String,
    reputation: i64,
    capability: Capability,
    priority: Priority,
    start_ts: u64,
    expected_secs: u64,
}

#[derive(Clone, Eq, PartialEq)]
struct ActiveRepEntry {
    reputation: i64,
    job_id: String,
}

impl Ord for ActiveRepEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.reputation.cmp(&other.reputation)
    }
}

impl PartialOrd for ActiveRepEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct QueuedJob {
    job_id: String,
    provider: String,
    capability: Capability,
    priority: Priority,
    enqueue_ts: u64,
    /// Cached effective priority scaled by 1000 for ordering.
    effective_priority: i64,
    expected_secs: u64,
}

impl QueuedJob {
    fn base_priority_value(&self) -> f64 {
        match self.priority {
            Priority::High => 0.0,
            Priority::Normal => 1.0,
            Priority::Low => 2.0,
        }
    }

    fn recompute_effective(&mut self) {
        let age = current_ts().saturating_sub(self.enqueue_ts) as f64;
        let rate = aging_rate();
        let max_boost = max_priority_boost();
        let boost = (age * rate).min(max_boost);
        let eff = (self.base_priority_value() - boost) * 1000.0;
        self.effective_priority = eff.round() as i64;
    }
}

impl Ord for QueuedJob {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.effective_priority
            .cmp(&other.effective_priority)
            .reverse()
            .then_with(|| other.enqueue_ts.cmp(&self.enqueue_ts))
    }
}

impl PartialOrd for QueuedJob {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for QueuedJob {
    fn eq(&self, other: &Self) -> bool {
        self.job_id == other.job_id && self.provider == other.provider
    }
}

impl Eq for QueuedJob {}

impl SchedulerState {
    fn register_offer(
        &mut self,
        provider: &str,
        capability: Capability,
        reputation: i64,
        price_per_unit: u64,
        multiplier: f64,
    ) {
        let min = f64::from_bits(REPUTATION_MULT_MIN.load(Ordering::Relaxed));
        let max = f64::from_bits(REPUTATION_MULT_MAX.load(Ordering::Relaxed));
        let clamped = multiplier.clamp(min, max);
        self.offers.insert(
            provider.to_owned(),
            OfferEntry {
                capability: capability.clone(),
                reputation,
                price_per_unit,
                multiplier: clamped,
            },
        );
        if PREEMPT_ENABLED.load(Ordering::Relaxed) {
            self.maybe_preempt(provider, &capability, reputation);
        }
    }

    fn maybe_preempt(&mut self, new_provider: &str, cap: &Capability, new_rep: i64) {
        let min_delta = PREEMPT_MIN_DELTA.load(Ordering::Relaxed);
        loop {
            let job_id = match self.active_by_rep.peek() {
                Some(Reverse(entry)) => entry.job_id.clone(),
                None => break,
            };
            let stale = match self.active.get(&job_id) {
                Some(assign) => {
                    if new_rep - assign.reputation < min_delta || !cap.matches(&assign.capability) {
                        break;
                    }
                    false
                }
                None => true,
            };
            if stale {
                self.active_by_rep.pop();
                continue;
            }
            if self.preempt(&job_id, new_provider, new_rep) {
                self.active_by_rep.pop();
                self.active_by_rep.push(Reverse(ActiveRepEntry {
                    reputation: new_rep,
                    job_id,
                }));
            }
            break;
        }
    }

    fn match_job(&mut self, need: &Capability) -> Option<String> {
        let mut best: Option<(String, u64)> = None;
        let mut mem_insufficient = false;
        let mut acc_mem_insufficient = false;
        for (prov, entry) in &self.offers {
            if let Some(req_gpu) = &need.gpu {
                if entry.capability.gpu.as_deref() == Some(req_gpu.as_str())
                    && entry.capability.gpu_memory_mb < need.gpu_memory_mb
                {
                    mem_insufficient = true;
                }
            }
            if let Some(req_acc) = &need.accelerator {
                if entry.capability.accelerator.as_ref() == Some(req_acc)
                    && entry.capability.accelerator_memory_mb < need.accelerator_memory_mb
                {
                    acc_mem_insufficient = true;
                }
            }
            if entry.capability.matches(need) {
                let rep = *self.reputation.get(prov).unwrap_or(&entry.reputation);
                if rep >= 0 {
                    let mut eff = entry.price_per_unit as f64 * entry.multiplier;
                    if need.accelerator.is_some() {
                        eff *= ACCELERATOR_PREMIUM;
                    }
                    let eff = eff.round() as u64;
                    match best {
                        Some((_, best_eff)) if eff >= best_eff => {}
                        _ => best = Some((prov.clone(), eff)),
                    }
                }
            }
        }
        let mut outcome = MatchOutcome::CapabilityMismatch;
        if let Some((prov, eff)) = &best {
            let cap_label = if let Some(g) = &self.offers[prov].capability.gpu {
                g.clone()
            } else if let Some(a) = &self.offers[prov].capability.accelerator {
                format!("{:?}", a)
            } else {
                "cpu".to_string()
            };
            self.utilization
                .entry(cap_label)
                .and_modify(|v| *v += 1)
                .or_insert(1);
            self.active_jobs += 1;
            adjust_thread_pool(self.active_jobs);
            outcome = MatchOutcome::Success;
            self.last_effective_price = Some(*eff);
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                {
                    telemetry::SCHEDULER_MATCH_TOTAL
                        .with_label_values(&["success"])
                        .inc();
                    telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
                    telemetry::SCHEDULER_EFFECTIVE_PRICE
                        .with_label_values(&[prov])
                        .set(*eff as i64);
                }
            }
        } else if mem_insufficient || acc_mem_insufficient {
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_MATCH_TOTAL
                    .with_label_values(&["capability_mismatch"])
                    .inc();
            }
            if need.accelerator.is_some() && SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_ACCELERATOR_MISS_TOTAL.inc();
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
                if need.accelerator.is_some() {
                    #[cfg(feature = "telemetry")]
                    telemetry::SCHEDULER_ACCELERATOR_MISS_TOTAL.inc();
                }
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
            let mut store = reputation_store();
            store.adjust(provider, 1);
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
            }
        }
    }

    fn record_failure(&mut self, provider: &str) {
        let rep = self.reputation.entry(provider.to_owned()).or_insert(0);
        *rep -= 1;
        {
            let mut store = reputation_store();
            store.adjust(provider, -1);
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
            }
        }
    }

    fn record_accelerator_success(&mut self, provider: &str) {
        let rep = self.reputation.entry(provider.to_owned()).or_insert(0);
        *rep += 1;
        {
            let mut store = reputation_store();
            store.adjust(provider, 1);
        }
        #[cfg(feature = "telemetry")]
        {
            telemetry::REPUTATION_ADJUST_TOTAL
                .with_label_values(&["accelerator_success"])
                .inc();
            telemetry::PROVIDER_REPUTATION_SCORE
                .with_label_values(&[provider])
                .set(*rep);
            telemetry::SCHEDULER_REPUTATION_SCORE.observe(*rep as f64);
        }
    }

    fn record_accelerator_failure(&mut self, provider: &str) {
        let rep = self.reputation.entry(provider.to_owned()).or_insert(0);
        *rep -= 1;
        {
            let mut store = reputation_store();
            store.adjust(provider, -1);
        }
        #[cfg(feature = "telemetry")]
        {
            telemetry::REPUTATION_ADJUST_TOTAL
                .with_label_values(&["accelerator_failure"])
                .inc();
            telemetry::PROVIDER_REPUTATION_SCORE
                .with_label_values(&[provider])
                .set(*rep);
            telemetry::SCHEDULER_REPUTATION_SCORE.observe(*rep as f64);
        }
    }

    fn end_job(&mut self, job_id: &str) {
        if let Some(a) = self.active.remove(job_id) {
            if self.active_jobs > 0 {
                self.active_jobs -= 1;
                adjust_thread_pool(self.active_jobs);
            }
            if a.priority == Priority::Low && self.active_low > 0 {
                self.active_low -= 1;
            }
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
            }
        }
        self.try_start_jobs();
    }

    fn preempt(&mut self, job_id: &str, new_provider: &str, new_rep: i64) -> bool {
        if !PREEMPT_ENABLED.load(Ordering::Relaxed) {
            return false;
        }
        let min_delta = PREEMPT_MIN_DELTA.load(Ordering::Relaxed);
        if let Some(current) = self.active.get(job_id).cloned() {
            if new_rep - current.reputation >= min_delta {
                courier::halt_job(job_id);
                match courier::handoff_job(job_id, new_provider) {
                    Ok(()) => {
                        self.reputation
                            .entry(new_provider.to_owned())
                            .or_insert(new_rep);
                        self.active.insert(
                            job_id.to_owned(),
                            ActiveAssignment {
                                provider: new_provider.to_owned(),
                                reputation: new_rep,
                                capability: current.capability.clone(),
                                priority: current.priority,
                                start_ts: current_ts(),
                                expected_secs: current.expected_secs,
                            },
                        );
                        self.active_by_rep.push(Reverse(ActiveRepEntry {
                            reputation: new_rep,
                            job_id: job_id.to_owned(),
                        }));
                        self.record_failure(&current.provider);
                        persist_cancellation(job_id, CancelReason::Preempted);
                        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                            #[cfg(feature = "telemetry")]
                            telemetry::SCHEDULER_CANCEL_TOTAL
                                .with_label_values(&["preempted"])
                                .inc();
                        }
                        self.preempt_total += 1;
                        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                            #[cfg(feature = "telemetry")]
                            telemetry::SCHEDULER_PREEMPT_TOTAL
                                .with_label_values(&["success"])
                                .inc();
                        }
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        diagnostics::tracing::info!(job_id, old = %current.provider, new = new_provider, "preempted job");
                        true
                    }
                    Err(_) => {
                        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                            #[cfg(feature = "telemetry")]
                            telemetry::SCHEDULER_PREEMPT_TOTAL
                                .with_label_values(&["handoff_failed"])
                                .inc();
                        }
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        diagnostics::tracing::warn!(job_id, old = %current.provider, new = new_provider, "handoff failed");
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    fn cancel_job(&mut self, job_id: &str, provider: &str, reason: CancelReason) -> bool {
        let accel = match self.active.get(job_id) {
            Some(a) => a.capability.accelerator.clone(),
            None => return false,
        };
        self.end_job(job_id);
        match reason {
            CancelReason::Client | CancelReason::ClientTimeout => {
                self.record_success(provider);
                if accel.is_some() {
                    self.record_accelerator_success(provider);
                }
            }
            CancelReason::Provider | CancelReason::ProviderFault => {
                self.record_failure(provider);
                if accel.is_some() {
                    self.record_accelerator_failure(provider);
                    if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                        #[cfg(feature = "telemetry")]
                        telemetry::SCHEDULER_ACCELERATOR_FAIL_TOTAL.inc();
                    }
                }
            }
            CancelReason::Preempted => {}
        }
        persist_cancellation(job_id, reason);
        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
            #[cfg(feature = "telemetry")]
            telemetry::SCHEDULER_CANCEL_TOTAL
                .with_label_values(&[reason.as_str()])
                .inc();
        }
        true
    }

    fn active_provider(&self, job_id: &str) -> Option<String> {
        self.active.get(job_id).map(|a| a.provider.clone())
    }

    fn job_requirements(&self, job_id: &str) -> Option<Capability> {
        self.active.get(job_id).map(|a| a.capability.clone())
    }

    fn provider_capability(&self, provider: &str) -> Option<Capability> {
        self.offers.get(provider).map(|o| o.capability.clone())
    }

    fn job_duration(&self, job_id: &str) -> Option<(u64, u64)> {
        self.active.get(job_id).map(|a| {
            let actual = current_ts().saturating_sub(a.start_ts);
            (a.expected_secs, actual)
        })
    }

    fn metrics(&self) -> Value {
        json!({
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
        let mut queued_high = 0;
        let mut queued_normal = 0;
        let mut queued_low = 0;
        let mut pending_jobs = Vec::new();
        for q in self.pending.iter() {
            match q.priority {
                Priority::High => queued_high += 1,
                Priority::Normal => queued_normal += 1,
                Priority::Low => queued_low += 1,
            }
            pending_jobs.push(PendingJob {
                job_id: q.job_id.clone(),
                priority: q.priority,
                effective_priority: q.effective_priority as f64 / 1000.0,
            });
        }
        SchedulerStats {
            success,
            capability_mismatch,
            reputation_failure,
            preemptions: self.preempt_total,
            active_jobs: self.active_jobs,
            utilization: self.utilization.clone(),
            effective_price: self.last_effective_price,
            queued_high,
            queued_normal,
            queued_low,
            priority_miss: self.priority_miss_total,
            pending: pending_jobs,
        }
    }

    fn enqueue_job(
        &mut self,
        job_id: &str,
        provider: &str,
        cap: Capability,
        priority: Priority,
        expected_secs: u64,
    ) {
        let now = current_ts();
        let mut job = QueuedJob {
            job_id: job_id.to_owned(),
            provider: provider.to_owned(),
            capability: cap,
            priority,
            enqueue_ts: now,
            effective_priority: 0,
            expected_secs,
        };
        job.recompute_effective();
        self.pending.push(job);
        self.persist_pending();
        self.try_start_jobs();
    }

    fn try_start_jobs(&mut self) {
        self.rebuild_pending();
        let cap_pct = LOW_PRIORITY_CAP_PCT.load(Ordering::Relaxed);
        while let Some(job) = self.pending.peek() {
            if job.priority == Priority::Low {
                if self.active_jobs > 0
                    && (self.active_low + 1) * 100 > cap_pct * (self.active_jobs + 1)
                {
                    break;
                }
            }
            let job = self.pending.pop().unwrap();
            let rep = *self.reputation.get(&job.provider).unwrap_or(&0);
            if job.priority == Priority::Low {
                self.active_low += 1;
            }
            let wait = current_ts().saturating_sub(job.enqueue_ts);
            let base = job.base_priority_value();
            let eff = job.effective_priority as f64 / 1000.0;
            if base - eff > 0.0 && SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_PRIORITY_BOOST_TOTAL.inc();
            }
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_JOB_AGE_SECONDS.observe(wait as f64);
            }
            if job.priority != Priority::Low && wait > PRIORITY_MISS_SECS {
                self.priority_miss_total += 1;
                if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                    #[cfg(feature = "telemetry")]
                    telemetry::SCHEDULER_PRIORITY_MISS_TOTAL.inc();
                }
            }
            self.active.insert(
                job.job_id.clone(),
                ActiveAssignment {
                    provider: job.provider.clone(),
                    reputation: rep,
                    capability: job.capability.clone(),
                    priority: job.priority,
                    start_ts: current_ts(),
                    expected_secs: job.expected_secs,
                },
            );
            courier::reserve_resources(&job.job_id);
            self.active_by_rep.push(Reverse(ActiveRepEntry {
                reputation: rep,
                job_id: job.job_id.clone(),
            }));
            self.active_jobs += 1;
            if job.capability.accelerator.is_some()
                && SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed)
            {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_ACCELERATOR_UTIL_TOTAL.inc();
            }
            if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
                #[cfg(feature = "telemetry")]
                telemetry::SCHEDULER_ACTIVE_JOBS.set(self.active_jobs as i64);
            }
            self.persist_pending();
        }
    }

    fn rebuild_pending(&mut self) {
        let mut jobs: Vec<_> = self.pending.drain().collect();
        for j in &mut jobs {
            j.recompute_effective();
        }
        for j in jobs {
            self.pending.push(j);
        }
    }

    fn persist_pending(&self) {
        let path = pending_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = json::to_vec(&self.pending.clone().into_vec()) {
            let _ = fs::write(path, json);
        }
    }
}

static SCHEDULER: Lazy<MutexT<SchedulerState>> = Lazy::new(|| {
    let rep = {
        let mut store = reputation_store();
        store.decay(provider_reputation_decay(), provider_reputation_retention());
        store
            .data
            .iter()
            .map(|(k, v)| (k.clone(), v.score))
            .collect()
    };
    mutex(SchedulerState {
        offers: HashMap::new(),
        utilization: HashMap::new(),
        reputation: rep,
        recent: VecDeque::new(),
        active_jobs: 0,
        active: HashMap::new(),
        preempt_total: 0,
        last_effective_price: None,
        pending: load_pending(),
        active_by_rep: BinaryHeap::new(),
        active_low: 0,
        priority_miss_total: 0,
    })
});

fn scheduler() -> MutexGuard<'static, SchedulerState> {
    SCHEDULER.guard()
}

#[derive(Serialize, Deserialize, Clone)]
struct ReputationEntry {
    score: i64,
    last_update: u64,
    #[serde(default)]
    epoch: u64,
}

#[derive(Default)]
pub struct ReputationStore {
    path: PathBuf,
    data: HashMap<String, ReputationEntry>,
}

impl ReputationStore {
    pub fn load(path: PathBuf) -> Self {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(data) = json::from_slice(&bytes) {
                return Self { path, data };
            }
        }
        Self {
            path,
            data: HashMap::new(),
        }
    }

    fn save(&self) {
        if let Ok(json) = json::to_vec(&self.data) {
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
                epoch: now,
            });
        entry.score += delta;
        entry.last_update = now;
        entry.epoch = now;
        self.save();
    }

    pub fn get(&self, provider: &str) -> i64 {
        self.data.get(provider).map(|e| e.score).unwrap_or(0)
    }

    /// Merge a gossiped reputation entry, returning true if applied.
    pub fn merge(&mut self, provider: &str, score: i64, epoch: u64) -> bool {
        const MAX_SCORE: i64 = 1_000;
        if score.abs() > MAX_SCORE {
            return false;
        }
        let entry = self
            .data
            .entry(provider.to_string())
            .or_insert(ReputationEntry {
                score: 0,
                last_update: current_ts(),
                epoch: 0,
            });
        if epoch <= entry.epoch {
            return false;
        }
        entry.score = score;
        entry.epoch = epoch;
        entry.last_update = current_ts();
        self.save();
        true
    }

    /// Snapshot all reputation entries for gossiping.
    pub fn snapshot(&self) -> Vec<crate::net::ReputationUpdate> {
        self.data
            .iter()
            .map(|(p, e)| crate::net::ReputationUpdate {
                provider_id: p.clone(),
                reputation_score: e.score,
                epoch: e.epoch,
            })
            .collect()
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

static REPUTATION_STORE: Lazy<MutexT<ReputationStore>> = Lazy::new(|| {
    let path = reputation_db_path();
    mutex(ReputationStore::load(path))
});

fn reputation_store() -> MutexGuard<'static, ReputationStore> {
    REPUTATION_STORE.guard()
}

static PROVIDER_REPUTATION_DECAY: AtomicU64 = AtomicU64::new(f64::to_bits(0.05));
static PROVIDER_REPUTATION_RETENTION: AtomicU64 = AtomicU64::new(7 * 24 * 60 * 60);
static SCHEDULER_METRICS_ENABLED: AtomicBool = AtomicBool::new(true);
static PREEMPT_ENABLED: AtomicBool = AtomicBool::new(false);
static PREEMPT_MIN_DELTA: AtomicI64 = AtomicI64::new(10);
static REPUTATION_MULT_MIN: AtomicU64 = AtomicU64::new((0.5f64).to_bits());
static REPUTATION_MULT_MAX: AtomicU64 = AtomicU64::new((1.0f64).to_bits());
static LOW_PRIORITY_CAP_PCT: AtomicU64 = AtomicU64::new(50);
static REPUTATION_GOSSIP_ENABLED: AtomicBool = AtomicBool::new(true);
static AGING_RATE: AtomicU64 = AtomicU64::new((0.001f64).to_bits());
static MAX_PRIORITY_BOOST: AtomicU64 = AtomicU64::new((1.0f64).to_bits());

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

pub fn set_preempt_enabled(val: bool) {
    PREEMPT_ENABLED.store(val, Ordering::Relaxed);
}

pub fn set_preempt_min_delta(delta: i64) {
    PREEMPT_MIN_DELTA.store(delta, Ordering::Relaxed);
}

pub fn set_low_priority_cap_pct(pct: u8) {
    LOW_PRIORITY_CAP_PCT.store(pct as u64, Ordering::Relaxed);
}

pub fn set_reputation_multiplier_bounds(min: f64, max: f64) {
    REPUTATION_MULT_MIN.store(min.to_bits(), Ordering::Relaxed);
    REPUTATION_MULT_MAX.store(max.to_bits(), Ordering::Relaxed);
}

pub fn set_reputation_gossip_enabled(val: bool) {
    REPUTATION_GOSSIP_ENABLED.store(val, Ordering::Relaxed);
}

pub fn reputation_gossip_enabled() -> bool {
    REPUTATION_GOSSIP_ENABLED.load(Ordering::Relaxed)
}

pub fn reputation_snapshot() -> Vec<crate::net::ReputationUpdate> {
    reputation_store().snapshot()
}

fn lookup_cancellation(job_id: &str) -> Option<String> {
    let path = cancel_log_path();
    if let Ok(contents) = fs::read_to_string(path) {
        for line in contents.lines().rev() {
            let mut parts = line.split_whitespace();
            if let (Some(id), Some(reason)) = (parts.next(), parts.next()) {
                if id == job_id {
                    return Some(reason.to_string());
                }
            }
        }
    }
    None
}

pub fn job_status(job_id: &str) -> Value {
    let sched = scheduler();
    if sched.active.contains_key(job_id) {
        json!({"status": "active"})
    } else if sched.pending.iter().any(|j| j.job_id == job_id) {
        json!({"status": "queued"})
    } else if let Some(reason) = lookup_cancellation(job_id) {
        json!({"status": "canceled", "reason": reason})
    } else {
        json!({"status": "unknown"})
    }
}

fn aging_rate() -> f64 {
    f64::from_bits(AGING_RATE.load(Ordering::Relaxed))
}

fn max_priority_boost() -> f64 {
    f64::from_bits(MAX_PRIORITY_BOOST.load(Ordering::Relaxed))
}

pub fn set_aging_rate(rate: f64) {
    AGING_RATE.store(rate.to_bits(), Ordering::Relaxed);
}

pub fn set_max_priority_boost(v: f64) {
    MAX_PRIORITY_BOOST.store(v.to_bits(), Ordering::Relaxed);
}

fn pending_path() -> PathBuf {
    std::env::var("TB_PENDING_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("pending_jobs.json")
        })
}

fn load_pending() -> BinaryHeap<QueuedJob> {
    if let Ok(bytes) = fs::read(pending_path()) {
        if let Ok(mut jobs) = json::from_slice::<Vec<QueuedJob>>(&bytes) {
            for j in &mut jobs {
                j.recompute_effective();
            }
            let mut heap = BinaryHeap::new();
            for j in jobs {
                heap.push(j);
            }
            return heap;
        }
    }
    BinaryHeap::new()
}

pub fn merge_reputation(provider: &str, score: i64, epoch: u64) -> bool {
    reputation_store().merge(provider, score, epoch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aging_reorders_jobs() {
        let mut q1 = QueuedJob {
            job_id: "low".into(),
            provider: "p1".into(),
            capability: Capability::default(),
            priority: Priority::Low,
            enqueue_ts: current_ts() - 10,
            effective_priority: 0,
            expected_secs: 0,
        };
        let mut q2 = QueuedJob {
            job_id: "high".into(),
            provider: "p2".into(),
            capability: Capability::default(),
            priority: Priority::High,
            enqueue_ts: current_ts(),
            effective_priority: 0,
            expected_secs: 0,
        };
        set_aging_rate(1.0);
        set_max_priority_boost(5.0);
        q1.recompute_effective();
        q2.recompute_effective();
        let mut heap = BinaryHeap::new();
        heap.push(q2.clone());
        heap.push(q1.clone());
        let top = heap.pop().unwrap();
        assert_eq!(top.job_id, "low");
    }

    #[test]
    fn reputation_gossip_roundtrip() {
        reset_for_test();
        record_success("peer1");
        let snapshot = reputation_snapshot();
        reset_for_test();
        assert_eq!(reputation_get("peer1"), 0);
        for entry in snapshot {
            merge_reputation(&entry.provider_id, entry.reputation_score, entry.epoch);
        }
        assert_eq!(reputation_get("peer1"), 1);
    }
}

pub fn validate_multiplier(m: f64) -> bool {
    let min = f64::from_bits(REPUTATION_MULT_MIN.load(Ordering::Relaxed));
    let max = f64::from_bits(REPUTATION_MULT_MAX.load(Ordering::Relaxed));
    m >= min && m <= max
}

fn reputation_db_path() -> PathBuf {
    std::env::var("TB_REPUTATION_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("reputation.json")
        })
}

fn cancel_log_path() -> PathBuf {
    std::env::var("TB_CANCEL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            paths::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".the_block")
                .join("cancellations.log")
        })
}

fn persist_cancellation(job_id: &str, reason: CancelReason) {
    let path = cancel_log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {}", job_id, reason.as_str());
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

pub fn register_offer(
    provider: &str,
    capability: Capability,
    reputation: i64,
    price_per_unit: u64,
    multiplier: f64,
) {
    scheduler().register_offer(provider, capability, reputation, price_per_unit, multiplier);
}

pub fn match_offer(need: &Capability) -> Option<String> {
    #[cfg(feature = "telemetry")]
    let start = std::time::Instant::now();
    let res = scheduler().match_job(need);
    if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
        #[cfg(feature = "telemetry")]
        telemetry::SCHEDULER_MATCH_LATENCY_SECONDS.observe(start.elapsed().as_secs_f64());
    }
    res
}

pub fn start_job_with_expected(
    job_id: &str,
    provider: &str,
    cap: Capability,
    priority: Priority,
    expected_secs: u64,
) {
    scheduler().enqueue_job(job_id, provider, cap, priority, expected_secs);
}

pub fn start_job_with_priority(job_id: &str, provider: &str, cap: Capability, priority: Priority) {
    start_job_with_expected(job_id, provider, cap, priority, 0);
}

pub fn start_job(job_id: &str, provider: &str, cap: Capability) {
    start_job_with_priority(job_id, provider, cap, Priority::Normal);
}

pub fn start_job_expected(job_id: &str, provider: &str, cap: Capability, expected_secs: u64) {
    start_job_with_expected(job_id, provider, cap, Priority::Normal, expected_secs);
}

pub fn end_job(job_id: &str) {
    scheduler().end_job(job_id);
}

pub fn cancel_job(job_id: &str, provider: &str, reason: CancelReason) -> bool {
    scheduler().cancel_job(job_id, provider, reason)
}

pub fn try_preempt(job_id: &str, new_provider: &str, new_rep: i64) -> bool {
    scheduler().preempt(job_id, new_provider, new_rep)
}

pub fn active_provider(job_id: &str) -> Option<String> {
    scheduler().active_provider(job_id)
}

pub fn job_requirements(job_id: &str) -> Option<Capability> {
    scheduler().job_requirements(job_id)
}

pub fn provider_capability(provider: &str) -> Option<Capability> {
    scheduler().provider_capability(provider)
}

pub fn job_duration(job_id: &str) -> Option<(u64, u64)> {
    scheduler().job_duration(job_id)
}

pub fn record_success(provider: &str) {
    scheduler().record_success(provider);
}

pub fn record_accelerator_success(provider: &str) {
    scheduler().record_accelerator_success(provider);
}

pub fn record_accelerator_failure(provider: &str) {
    scheduler().record_accelerator_failure(provider);
}

pub fn record_failure(provider: &str) {
    scheduler().record_failure(provider);
}

pub fn metrics() -> Value {
    scheduler().metrics()
}

pub fn stats() -> SchedulerStats {
    scheduler().stats()
}

pub fn reputation_get(provider: &str) -> i64 {
    reputation_store().get(provider)
}

pub fn reset_for_test() {
    {
        let mut s = scheduler();
        s.offers.clear();
        s.utilization.clear();
        s.reputation.clear();
        s.recent.clear();
        s.active_jobs = 0;
        s.active.clear();
        s.preempt_total = 0;
        s.last_effective_price = None;
        if SCHEDULER_METRICS_ENABLED.load(Ordering::Relaxed) {
            #[cfg(feature = "telemetry")]
            telemetry::SCHEDULER_ACTIVE_JOBS.set(0);
        }
    }
    reputation_store().data.clear();
    PREEMPT_ENABLED.store(false, Ordering::Relaxed);
    PREEMPT_MIN_DELTA.store(10, Ordering::Relaxed);
    REPUTATION_MULT_MIN.store((0.5f64).to_bits(), Ordering::Relaxed);
    REPUTATION_MULT_MAX.store((1.0f64).to_bits(), Ordering::Relaxed);
}
