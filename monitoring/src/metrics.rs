use foundation_metrics::{self, Recorder, RecorderInstallError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

const MONITORING_SNAPSHOT_SUCCESS_TOTAL: &str = "monitoring_snapshot_success_total";
const MONITORING_SNAPSHOT_ERROR_TOTAL: &str = "monitoring_snapshot_error_total";

#[derive(Default)]
struct MonitoringRecorder {
    success: AtomicU64,
    error: AtomicU64,
}

impl MonitoringRecorder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            success: AtomicU64::new(0),
            error: AtomicU64::new(0),
        })
    }

    fn snapshot(&self) -> MonitoringMetrics {
        MonitoringMetrics {
            success_total: self.success.load(Ordering::Relaxed),
            error_total: self.error.load(Ordering::Relaxed),
        }
    }

    fn reset(&self) {
        self.success.store(0, Ordering::Relaxed);
        self.error.store(0, Ordering::Relaxed);
    }

    fn increment_success(&self, value: u64) {
        self.success.fetch_add(value, Ordering::Relaxed);
    }

    fn increment_error(&self, value: u64) {
        self.error.fetch_add(value, Ordering::Relaxed);
    }
}

impl Recorder for MonitoringRecorder {
    fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]) {
        if !labels.is_empty() {
            return;
        }
        if !value.is_finite() || value < 0.0 {
            return;
        }
        let delta = value.round() as u64;
        match name {
            MONITORING_SNAPSHOT_SUCCESS_TOTAL => self.increment_success(delta),
            MONITORING_SNAPSHOT_ERROR_TOTAL => self.increment_error(delta),
            _ => {}
        }
    }

    fn record_histogram(&self, _name: &str, _value: f64, _labels: &[(String, String)]) {}

    fn record_gauge(&self, _name: &str, _value: f64, _labels: &[(String, String)]) {}
}

static MONITORING_RECORDER: OnceLock<Arc<MonitoringRecorder>> = OnceLock::new();

/// Installs the global monitoring recorder if one is not present.
pub fn ensure_monitoring_recorder() {
    MONITORING_RECORDER.get_or_init(|| {
        let recorder = MonitoringRecorder::new();
        let _ = foundation_metrics::install_shared_recorder(recorder.clone());
        recorder
    });
}

/// Installs a fresh monitoring recorder, returning an error if a recorder is
/// already active.
pub fn install_monitoring_recorder() -> Result<(), RecorderInstallError> {
    let recorder = MonitoringRecorder::new();
    foundation_metrics::install_shared_recorder(recorder.clone())?;
    let _ = MONITORING_RECORDER.set(recorder);
    Ok(())
}

/// Returns the current metrics snapshot if a recorder is installed.
pub fn monitoring_metrics() -> Option<MonitoringMetrics> {
    MONITORING_RECORDER
        .get()
        .map(|recorder| recorder.snapshot())
}

/// Resets the monitoring counters. Primarily used from tests to obtain a clean
/// baseline between assertions.
pub fn reset_monitoring_metrics() {
    if let Some(recorder) = MONITORING_RECORDER.get() {
        recorder.reset();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MonitoringMetrics {
    pub success_total: u64,
    pub error_total: u64,
}

pub fn record_snapshot_success() {
    foundation_metrics::increment_counter!(MONITORING_SNAPSHOT_SUCCESS_TOTAL);
}

pub fn record_snapshot_error() {
    foundation_metrics::increment_counter!(MONITORING_SNAPSHOT_ERROR_TOTAL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_tracks_success_and_error_counts() {
        ensure_monitoring_recorder();
        reset_monitoring_metrics();
        record_snapshot_success();
        record_snapshot_error();
        record_snapshot_error();
        let snapshot = monitoring_metrics().expect("recorder installed");
        assert_eq!(snapshot.success_total, 1);
        assert_eq!(snapshot.error_total, 2);
        reset_monitoring_metrics();
    }
}
