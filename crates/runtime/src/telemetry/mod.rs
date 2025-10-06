use std::fmt::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Errors returned by the telemetry registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// Attempted to register a metric with a name that already exists.
    DuplicateMetric(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryError::DuplicateMetric(name) => {
                write!(f, "duplicate metric name: {name}")
            }
        }
    }
}

impl std::error::Error for TelemetryError {}

#[derive(Clone)]
pub struct Registry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    metrics: RwLock<Vec<Arc<Metric>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                metrics: RwLock::new(Vec::new()),
            }),
        }
    }

    pub fn register_counter(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
    ) -> Result<Counter, TelemetryError> {
        self.register_metric(name.into(), help.into(), MetricKind::Counter)
            .map(Counter::from_metric)
    }

    pub fn register_gauge(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
    ) -> Result<Gauge, TelemetryError> {
        self.register_metric(name.into(), help.into(), MetricKind::Gauge)
            .map(Gauge::from_metric)
    }

    fn register_metric(
        &self,
        name: String,
        help: String,
        kind: MetricKind,
    ) -> Result<Arc<Metric>, TelemetryError> {
        let mut guard = self
            .inner
            .metrics
            .write()
            .expect("telemetry registry poisoned");
        if guard.iter().any(|metric| metric.name == name) {
            return Err(TelemetryError::DuplicateMetric(name));
        }
        let metric = Arc::new(Metric::new(name, help, kind));
        guard.push(metric.clone());
        Ok(metric)
    }

    /// Render all registered metrics to the Prometheus text exposition format.
    pub fn render(&self) -> String {
        let guard = self
            .inner
            .metrics
            .read()
            .expect("telemetry registry poisoned");
        let mut output = String::new();
        for metric in guard.iter() {
            metric.render(&mut output);
        }
        output
    }

    /// Capture an immutable snapshot of the registered metrics.
    pub fn snapshot(&self) -> Vec<MetricSnapshot> {
        let guard = self
            .inner
            .metrics
            .read()
            .expect("telemetry registry poisoned");
        guard.iter().map(|metric| metric.snapshot()).collect()
    }
}

#[derive(Clone)]
pub struct Counter {
    inner: Arc<Metric>,
}

impl fmt::Debug for Counter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Counter")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl Counter {
    fn from_metric(metric: Arc<Metric>) -> Self {
        Self { inner: metric }
    }

    pub fn inc(&self) {
        self.inc_by(1);
    }

    pub fn inc_by(&self, value: u64) {
        if let MetricValue::Counter(counter) = &self.inner.value {
            counter.fetch_add(value, Ordering::Relaxed);
        } else {
            unreachable!("counter wrapped non-counter metric");
        }
    }

    pub fn value(&self) -> u64 {
        if let MetricValue::Counter(counter) = &self.inner.value {
            counter.load(Ordering::Relaxed)
        } else {
            unreachable!("counter wrapped non-counter metric");
        }
    }
}

#[derive(Clone)]
pub struct Gauge {
    inner: Arc<Metric>,
}

impl fmt::Debug for Gauge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gauge")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl Gauge {
    fn from_metric(metric: Arc<Metric>) -> Self {
        Self { inner: metric }
    }

    pub fn set(&self, value: f64) {
        if let MetricValue::Gauge(gauge) = &self.inner.value {
            gauge.store(value.to_bits(), Ordering::Relaxed);
        } else {
            unreachable!("gauge wrapped non-gauge metric");
        }
    }

    pub fn add(&self, delta: f64) {
        if let MetricValue::Gauge(gauge) = &self.inner.value {
            let mut current = gauge.load(Ordering::Relaxed);
            loop {
                let value = f64::from_bits(current);
                let new_bits = (value + delta).to_bits();
                match gauge.compare_exchange(
                    current,
                    new_bits,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(next) => current = next,
                }
            }
        } else {
            unreachable!("gauge wrapped non-gauge metric");
        }
    }

    pub fn value(&self) -> f64 {
        if let MetricValue::Gauge(gauge) = &self.inner.value {
            f64::from_bits(gauge.load(Ordering::Relaxed))
        } else {
            unreachable!("gauge wrapped non-gauge metric");
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricSnapshot {
    pub name: String,
    pub help: String,
    pub kind: MetricKind,
    pub value: MetricValueSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetricValueSnapshot {
    Counter(u64),
    Gauge(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetricKind {
    Counter,
    Gauge,
}

struct Metric {
    name: String,
    help: String,
    kind: MetricKind,
    value: MetricValue,
}

enum MetricValue {
    Counter(AtomicU64),
    Gauge(AtomicU64),
}

impl Metric {
    fn new(name: String, help: String, kind: MetricKind) -> Self {
        let value = match kind {
            MetricKind::Counter => MetricValue::Counter(AtomicU64::new(0)),
            MetricKind::Gauge => MetricValue::Gauge(AtomicU64::new(0_f64.to_bits())),
        };
        Self {
            name,
            help,
            kind,
            value,
        }
    }

    fn render(&self, output: &mut String) {
        let _ = writeln!(output, "# HELP {} {}", self.name, self.help);
        let metric_type = match self.kind {
            MetricKind::Counter => "counter",
            MetricKind::Gauge => "gauge",
        };
        let _ = writeln!(output, "# TYPE {} {}", self.name, metric_type);
        let value = match &self.value {
            MetricValue::Counter(counter) => {
                MetricValueSnapshot::Counter(counter.load(Ordering::Relaxed))
            }
            MetricValue::Gauge(gauge) => {
                MetricValueSnapshot::Gauge(f64::from_bits(gauge.load(Ordering::Relaxed)))
            }
        };
        match value {
            MetricValueSnapshot::Counter(val) => {
                let _ = writeln!(output, "{} {}", self.name, val);
            }
            MetricValueSnapshot::Gauge(val) => {
                if val.fract() == 0.0 {
                    let _ = writeln!(output, "{} {:.0}", self.name, val);
                } else {
                    let _ = writeln!(output, "{} {}", self.name, val);
                }
            }
        }
    }

    fn snapshot(&self) -> MetricSnapshot {
        let value = match &self.value {
            MetricValue::Counter(counter) => {
                MetricValueSnapshot::Counter(counter.load(Ordering::Relaxed))
            }
            MetricValue::Gauge(gauge) => {
                MetricValueSnapshot::Gauge(f64::from_bits(gauge.load(Ordering::Relaxed)))
            }
        };
        MetricSnapshot {
            name: self.name.clone(),
            help: self.help.clone(),
            kind: self.kind.clone(),
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_increment_and_render() {
        let registry = Registry::new();
        let counter = registry
            .register_counter("example_total", "Example counter")
            .unwrap();
        counter.inc();
        counter.inc_by(4);
        let output = registry.render();
        assert!(output.contains("# HELP example_total Example counter"));
        assert!(output.contains("# TYPE example_total counter"));
        assert!(output.contains("example_total 5"));
    }

    #[test]
    fn gauges_store_f64_values() {
        let registry = Registry::new();
        let gauge = registry
            .register_gauge("temperature_celsius", "Temperature")
            .unwrap();
        gauge.set(42.5);
        gauge.add(0.5);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].name, "temperature_celsius");
        assert_eq!(snapshot[0].kind, MetricKind::Gauge);
        assert_eq!(snapshot[0].value, MetricValueSnapshot::Gauge(43.0));
    }

    #[test]
    fn duplicate_names_error() {
        let registry = Registry::new();
        registry
            .register_counter("dup_total", "first")
            .expect("first registration succeeds");
        let err = registry
            .register_counter("dup_total", "second")
            .expect_err("duplicate registration fails");
        assert_eq!(
            err,
            TelemetryError::DuplicateMetric("dup_total".to_string())
        );
    }
}
