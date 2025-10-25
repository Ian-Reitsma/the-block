use concurrency::Lazy;
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(feature = "telemetry")]
use foundation_serialization::Serialize;

#[cfg(feature = "telemetry")]
#[derive(Clone, Debug, Serialize)]
pub struct TelemetrySummary {
    pub seq: u64,
    pub timestamp: u64,
    pub sample_rate_ppm: u64,
    pub compaction_secs: u64,
    pub node_id: String,
    pub memory: HashMap<String, crate::telemetry::MemorySnapshot>,
    pub wrappers: crate::telemetry::WrapperSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ad_readiness: Option<foundation_telemetry::AdReadinessTelemetry>,
}

#[cfg(not(feature = "telemetry"))]
#[derive(Clone, Debug, Default)]
pub struct TelemetrySummary;

#[cfg(feature = "telemetry")]
static LAST_SUMMARY: Lazy<RwLock<Option<TelemetrySummary>>> = Lazy::new(|| RwLock::new(None));
#[cfg(not(feature = "telemetry"))]
static LAST_SUMMARY: Lazy<RwLock<Option<TelemetrySummary>>> = Lazy::new(|| RwLock::new(None));

static LAST_SEQ: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static NODE_LABEL: Lazy<String> =
    Lazy::new(|| env::var("TB_NODE_LABEL").unwrap_or_else(|_| "node".to_string()));

#[cfg(feature = "telemetry")]
pub fn spawn(interval_secs: u64) {
    if interval_secs == 0 {
        return;
    }
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(interval_secs));
        let seq = LAST_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let summary = build_summary(seq);
        if let Ok(mut guard) = LAST_SUMMARY.write() {
            *guard = Some(summary.clone());
        }
        crate::net::publish_telemetry_summary(summary);
    });
}

#[cfg(not(feature = "telemetry"))]
pub fn spawn(_interval_secs: u64) {}

#[cfg(feature = "telemetry")]
fn build_summary(seq: u64) -> TelemetrySummary {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let memory = crate::telemetry::READ_STATS
        .memory_snapshot_all()
        .into_iter()
        .map(|(component, snapshot)| (component.to_string(), snapshot))
        .collect::<HashMap<_, _>>();
    TelemetrySummary {
        seq,
        timestamp: ts,
        sample_rate_ppm: crate::telemetry::sample_rate_ppm(),
        compaction_secs: crate::telemetry::compaction_interval_secs(),
        node_id: NODE_LABEL.clone(),
        memory,
        wrappers: crate::telemetry::wrapper_metrics_snapshot(),
        ad_readiness: crate::ad_readiness::global_snapshot().map(|snapshot| {
            foundation_telemetry::AdReadinessTelemetry {
                ready: snapshot.ready,
                window_secs: snapshot.window_secs,
                min_unique_viewers: snapshot.min_unique_viewers,
                min_host_count: snapshot.min_host_count,
                min_provider_count: snapshot.min_provider_count,
                unique_viewers: snapshot.unique_viewers,
                host_count: snapshot.host_count,
                provider_count: snapshot.provider_count,
                blockers: snapshot.blockers,
                last_updated: snapshot.last_updated,
            }
        }),
    }
}

#[cfg(feature = "telemetry")]
pub fn latest() -> Option<TelemetrySummary> {
    LAST_SUMMARY.read().ok().and_then(|guard| guard.clone())
}

#[cfg(not(feature = "telemetry"))]
pub fn latest() -> Option<TelemetrySummary> {
    None
}

pub fn last_count() -> u64 {
    LAST_SEQ.load(Ordering::Relaxed)
}
