#![forbid(unsafe_code)]

use std::fmt;
use std::sync::{Arc, OnceLock};

/// Tuple representing a single metrics label key/value pair.
pub type LabelPair = (String, String);

/// Trait implemented by consumers that want to receive emitted metrics events.
pub trait Recorder: Send + Sync + 'static {
    /// Record a counter increment. The value represents the delta that should be added.
    fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]);

    /// Record a histogram sample.
    fn record_histogram(&self, name: &str, value: f64, labels: &[(String, String)]);

    /// Record the latest gauge value.
    fn record_gauge(&self, name: &str, value: f64, labels: &[(String, String)]);
}

/// Error returned when attempting to install more than one recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecorderInstallError;

impl fmt::Display for RecorderInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "metrics recorder already installed")
    }
}

impl std::error::Error for RecorderInstallError {}

type RecorderHandle = Arc<dyn Recorder>;

static RECORDER: OnceLock<RecorderHandle> = OnceLock::new();

/// Install the global recorder used by all emitted metrics.
///
/// Only a single recorder may be installed. Subsequent calls will return
/// [`RecorderInstallError`].
pub fn install_recorder<R>(recorder: R) -> Result<(), RecorderInstallError>
where
    R: Recorder,
{
    install_shared_recorder(Arc::new(recorder))
}

/// Install a previously shared recorder handle.
pub fn install_shared_recorder(recorder: Arc<dyn Recorder>) -> Result<(), RecorderInstallError> {
    RECORDER.set(recorder).map_err(|_| RecorderInstallError)
}

/// Returns `true` if a recorder has been installed.
pub fn recorder_installed() -> bool {
    RECORDER.get().is_some()
}

fn with_recorder<F>(mut labels: Vec<LabelPair>, mut f: F)
where
    F: FnMut(&dyn Recorder, &[(String, String)]),
{
    if let Some(recorder) = RECORDER.get() {
        f(recorder.as_ref(), &labels);
    }
    labels.clear();
}

/// Convert a label key into an owned [`String`].
pub fn label_key<T: ToString>(value: T) -> String {
    value.to_string()
}

/// Convert a label value into an owned [`String`].
pub fn label_value<T: ToString>(value: T) -> String {
    value.to_string()
}

/// Emit a counter increment with the provided value and labels.
pub fn record_counter(name: &str, value: f64, labels: Vec<LabelPair>) {
    with_recorder(labels, |recorder, labels| {
        recorder.increment_counter(name, value, labels);
    });
}

/// Emit a histogram sample with the provided labels.
pub fn record_histogram(name: &str, value: f64, labels: Vec<LabelPair>) {
    with_recorder(labels, |recorder, labels| {
        recorder.record_histogram(name, value, labels);
    });
}

/// Emit a gauge update with the provided labels.
pub fn record_gauge(name: &str, value: f64, labels: Vec<LabelPair>) {
    with_recorder(labels, |recorder, labels| {
        recorder.record_gauge(name, value, labels);
    });
}

#[macro_export]
macro_rules! increment_counter {
    ($name:expr $(,)?) => {{
        $crate::record_counter($name, 1.0, Vec::new());
    }};
    ($name:expr, $value:expr $(,)?) => {{
        let value = $value as f64;
        $crate::record_counter($name, value, Vec::new());
    }};
    ($name:expr $(, $key:expr => $value:expr )+ $(,)?) => {{
        let mut labels = Vec::<$crate::LabelPair>::new();
        $(
            labels.push(($crate::label_key($key), $crate::label_value($value)));
        )+
        $crate::record_counter($name, 1.0, labels);
    }};
    ($name:expr, $value:expr $(, $key:expr => $label:expr )+ $(,)?) => {{
        let value = $value as f64;
        let mut labels = Vec::<$crate::LabelPair>::new();
        $(
            labels.push(($crate::label_key($key), $crate::label_value($label)));
        )+
        $crate::record_counter($name, value, labels);
    }};
}

#[macro_export]
macro_rules! histogram {
    ($name:expr, $value:expr $(,)?) => {{
        let value = $value as f64;
        $crate::record_histogram($name, value, Vec::new());
    }};
    ($name:expr, $value:expr $(, $key:expr => $label:expr )+ $(,)?) => {{
        let value = $value as f64;
        let mut labels = Vec::<$crate::LabelPair>::new();
        $(
            labels.push(($crate::label_key($key), $crate::label_value($label)));
        )+
        $crate::record_histogram($name, value, labels);
    }};
}

#[macro_export]
macro_rules! gauge {
    ($name:expr, $value:expr $(,)?) => {{
        let value = $value as f64;
        $crate::record_gauge($name, value, Vec::new());
    }};
    ($name:expr, $value:expr $(, $key:expr => $label:expr )+ $(,)?) => {{
        let value = $value as f64;
        let mut labels = Vec::<$crate::LabelPair>::new();
        $(
            labels.push(($crate::label_key($key), $crate::label_value($label)));
        )+
        $crate::record_gauge($name, value, labels);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordedEvent {
        name: String,
        value: f64,
        labels: Vec<(String, String)>,
    }

    #[derive(Default)]
    struct TestRecorder {
        counters: Mutex<Vec<RecordedEvent>>,
        histograms: Mutex<Vec<RecordedEvent>>,
        gauges: Mutex<Vec<RecordedEvent>>,
    }

    impl Recorder for TestRecorder {
        fn increment_counter(&self, name: &str, value: f64, labels: &[(String, String)]) {
            let mut guard = self.counters.lock().expect("counter guard");
            guard.push(RecordedEvent {
                name: name.to_string(),
                value,
                labels: labels.to_vec(),
            });
        }

        fn record_histogram(&self, name: &str, value: f64, labels: &[(String, String)]) {
            let mut guard = self.histograms.lock().expect("histogram guard");
            guard.push(RecordedEvent {
                name: name.to_string(),
                value,
                labels: labels.to_vec(),
            });
        }

        fn record_gauge(&self, name: &str, value: f64, labels: &[(String, String)]) {
            let mut guard = self.gauges.lock().expect("gauge guard");
            guard.push(RecordedEvent {
                name: name.to_string(),
                value,
                labels: labels.to_vec(),
            });
        }
    }

    fn install_test_recorder() -> Arc<TestRecorder> {
        static INSTALLED: OnceLock<Arc<TestRecorder>> = OnceLock::new();
        INSTALLED
            .get_or_init(|| {
                let recorder = Arc::new(TestRecorder::default());
                let _ = install_shared_recorder(recorder.clone());
                recorder
            })
            .clone()
    }

    #[test]
    fn counter_macro_records_defaults() {
        let recorder = install_test_recorder();
        increment_counter!("test_counter");
        let guard = recorder.counters.lock().expect("counter guard");
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].name, "test_counter");
        assert_eq!(guard[0].value, 1.0);
        assert!(guard[0].labels.is_empty());
    }

    #[test]
    fn histogram_macro_records_labels() {
        let recorder = install_test_recorder();
        histogram!("test_histogram", 2.5, "foo" => "bar", "baz" => 42);
        let guard = recorder.histograms.lock().expect("histogram guard");
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].name, "test_histogram");
        assert_eq!(guard[0].value, 2.5);
        assert_eq!(guard[0].labels.len(), 2);
        assert_eq!(guard[0].labels[0], ("foo".to_string(), "bar".to_string()));
        assert_eq!(guard[0].labels[1], ("baz".to_string(), "42".to_string()));
    }

    #[test]
    fn gauge_macro_records_value() {
        let recorder = install_test_recorder();
        gauge!("test_gauge", 7, "role" => "primary");
        let guard = recorder.gauges.lock().expect("gauge guard");
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].value, 7.0);
        assert_eq!(
            guard[0].labels[0],
            ("role".to_string(), "primary".to_string())
        );
    }

    #[test]
    fn counter_macro_with_value_and_labels() {
        let recorder = install_test_recorder();
        increment_counter!("custom_counter", 4, "source" => "test");
        let guard = recorder.counters.lock().expect("counter guard");
        assert_eq!(guard.len(), 2);
        assert_eq!(guard[1].value, 4.0);
        assert_eq!(
            guard[1].labels[0],
            ("source".to_string(), "test".to_string())
        );
    }
}
