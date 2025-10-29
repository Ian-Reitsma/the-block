#![cfg(test)]

use foundation_metrics::Recorder;
use std::sync::{Arc, Mutex, OnceLock};

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct RecordedMetric {
    pub name: String,
    pub value: f64,
    pub labels: Vec<(String, String)>,
}

#[derive(Default)]
pub struct TestMetricsRecorder {
    counters: Mutex<Vec<RecordedMetric>>,
    histograms: Mutex<Vec<RecordedMetric>>,
    gauges: Mutex<Vec<RecordedMetric>>,
}

impl TestMetricsRecorder {
    pub fn install() -> Option<Arc<Self>> {
        static INSTANCE: OnceLock<Arc<TestMetricsRecorder>> = OnceLock::new();
        if let Some(existing) = INSTANCE.get() {
            return Some(existing.clone());
        }
        let recorder = Arc::new(TestMetricsRecorder::default());
        match foundation_metrics::install_shared_recorder(recorder.clone()) {
            Ok(()) => {
                let _ = INSTANCE.set(recorder.clone());
                Some(recorder)
            }
            Err(_) => None,
        }
    }

    pub fn reset(&self) {
        self.counters.lock().expect("counter guard").clear();
        self.histograms.lock().expect("hist guard").clear();
        self.gauges.lock().expect("gauge guard").clear();
    }

    pub fn counters(&self) -> Vec<RecordedMetric> {
        self.counters.lock().expect("counter guard").clone()
    }

    pub fn histograms(&self) -> Vec<RecordedMetric> {
        self.histograms.lock().expect("hist guard").clone()
    }

    pub fn gauges(&self) -> Vec<RecordedMetric> {
        self.gauges.lock().expect("gauge guard").clone()
    }
}

impl Recorder for TestMetricsRecorder {
    fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let mut guard = self.counters.lock().expect("counter guard");
        guard.push(RecordedMetric {
            name: name.to_string(),
            value,
            labels: labels.to_vec(),
        });
    }

    fn record_histogram(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let mut guard = self.histograms.lock().expect("hist guard");
        guard.push(RecordedMetric {
            name: name.to_string(),
            value,
            labels: labels.to_vec(),
        });
    }

    fn record_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
        let mut guard = self.gauges.lock().expect("gauge guard");
        guard.push(RecordedMetric {
            name: name.to_string(),
            value,
            labels: labels.to_vec(),
        });
    }
}
