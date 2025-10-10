use concurrency::DashMap;
use std::io::Write as IoWrite;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Errors returned by the telemetry registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// Attempted to register a metric with a name that already exists.
    DuplicateMetric(String),
    /// Metric definition was invalid.
    InvalidMetric(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryError::DuplicateMetric(name) => {
                write!(f, "duplicate metric name: {name}")
            }
            TelemetryError::InvalidMetric(msg) => write!(f, "invalid metric definition: {msg}"),
        }
    }
}

impl std::error::Error for TelemetryError {}

/// Errors returned when constructing metrics or performing metric operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricError {
    /// The provided label cardinality does not match the metric definition.
    InconsistentCardinality { expected: usize, actual: usize },
    /// Metric queried with a label set that has not been registered.
    MissingLabelSet,
    /// Histogram buckets were invalid.
    InvalidBuckets(&'static str),
}

impl std::fmt::Display for MetricError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricError::InconsistentCardinality { expected, actual } => {
                write!(
                    f,
                    "label cardinality mismatch: expected {expected}, received {actual}"
                )
            }
            MetricError::MissingLabelSet => write!(f, "requested label set is not registered"),
            MetricError::InvalidBuckets(msg) => write!(f, "invalid histogram buckets: {msg}"),
        }
    }
}

impl std::error::Error for MetricError {}

pub type Result<T> = std::result::Result<T, MetricError>;

pub const LABEL_REGISTRATION_ERR: &str = "telemetry label set not registered";

/// Trait implemented by all telemetry collectors registered in the registry.
pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    fn help(&self) -> &str;
    fn collect(&self) -> MetricFamily;
}

#[derive(Clone)]
pub struct Registry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    collectors: RwLock<Vec<Arc<dyn Collector>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

pub const TEXT_MIME: &str = "text/plain; charset=utf-8";

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                collectors: RwLock::new(Vec::new()),
            }),
        }
    }

    pub fn register_counter(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
    ) -> std::result::Result<Counter, TelemetryError> {
        let counter =
            Counter::new(name, help).map_err(|e| TelemetryError::InvalidMetric(e.to_string()))?;
        self.register(Box::new(counter.clone()))?;
        Ok(counter)
    }

    pub fn register_gauge(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
    ) -> std::result::Result<Gauge, TelemetryError> {
        let gauge = Gauge::new(name, help);
        self.register(Box::new(gauge.clone()))?;
        Ok(gauge)
    }

    pub fn register(
        &self,
        collector: Box<dyn Collector>,
    ) -> std::result::Result<(), TelemetryError> {
        let mut guard = self
            .inner
            .collectors
            .write()
            .expect("telemetry registry poisoned");
        if guard
            .iter()
            .any(|existing| existing.name() == collector.name())
        {
            return Err(TelemetryError::DuplicateMetric(
                collector.name().to_string(),
            ));
        }
        guard.push(Arc::from(collector));
        Ok(())
    }

    pub fn gather(&self) -> Vec<MetricFamily> {
        let guard = self
            .inner
            .collectors
            .read()
            .expect("telemetry registry poisoned");
        guard.iter().map(|c| c.collect()).collect()
    }

    /// Render all registered metrics into a UTF-8 text payload.
    pub fn render_bytes(&self) -> Vec<u8> {
        let families = self.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder
            .encode(&families, &mut buffer)
            .expect("encoding telemetry snapshot");
        buffer
    }

    /// Render all registered metrics to a UTF-8 string payload.
    pub fn render(&self) -> String {
        String::from_utf8(self.render_bytes()).expect("telemetry output must be valid utf8")
    }

    /// Capture an immutable snapshot of the registered metrics.
    pub fn snapshot(&self) -> Vec<MetricFamily> {
        self.gather()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricFamily {
    pub name: String,
    pub help: String,
    pub r#type: MetricType,
    pub samples: Vec<MetricSample>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricSample {
    pub labels: Vec<(String, String)>,
    pub value: MetricSampleValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetricSampleValue {
    Counter(u64),
    IntGauge(i64),
    Gauge(f64),
    Histogram {
        buckets: Vec<(f64, u64)>,
        count: u64,
        sum: f64,
    },
}

fn write_labels(labels: &[(String, String)], output: &mut String) {
    if labels.is_empty() {
        return;
    }
    output.push('{');
    for (idx, (key, value)) in labels.iter().enumerate() {
        if idx > 0 {
            output.push(',');
        }
        output.push_str(key);
        output.push_str("=\"");
        for ch in value.chars() {
            match ch {
                '\\' => output.push_str("\\\\"),
                '\n' => output.push_str("\\n"),
                '"' => output.push_str("\\\""),
                other => output.push(other),
            }
        }
        output.push('"');
    }
    output.push('}');
}

fn write_sample(
    name: &str,
    labels: &[(String, String)],
    value: &str,
    writer: &mut dyn IoWrite,
) -> std::io::Result<()> {
    let mut line = String::new();
    line.push_str(name);
    write_labels(labels, &mut line);
    line.push(' ');
    line.push_str(value);
    line.push('\n');
    writer.write_all(line.as_bytes())
}

pub trait Encoder {
    fn encode(&self, families: &[MetricFamily], writer: &mut dyn IoWrite) -> std::io::Result<()>;
}

pub struct TextEncoder;

impl TextEncoder {
    pub fn new() -> Self {
        Self
    }
}

impl Encoder for TextEncoder {
    fn encode(&self, families: &[MetricFamily], writer: &mut dyn IoWrite) -> std::io::Result<()> {
        for family in families {
            let help_line = format!("# HELP {} {}\n", family.name, family.help);
            writer.write_all(help_line.as_bytes())?;
            let type_label = match family.r#type {
                MetricType::Counter => "counter",
                MetricType::Gauge => "gauge",
                MetricType::Histogram => "histogram",
            };
            let type_line = format!("# TYPE {} {}\n", family.name, type_label);
            writer.write_all(type_line.as_bytes())?;
            for sample in &family.samples {
                match &sample.value {
                    MetricSampleValue::Counter(v) => {
                        write_sample(&family.name, &sample.labels, &v.to_string(), writer)?;
                    }
                    MetricSampleValue::IntGauge(v) => {
                        write_sample(&family.name, &sample.labels, &v.to_string(), writer)?;
                    }
                    MetricSampleValue::Gauge(v) => {
                        write_sample(&family.name, &sample.labels, &v.to_string(), writer)?;
                    }
                    MetricSampleValue::Histogram {
                        buckets,
                        count,
                        sum,
                    } => {
                        let mut cumulative = 0u64;
                        for (boundary, bucket_count) in buckets.iter() {
                            cumulative += *bucket_count;
                            let mut labels = sample.labels.clone();
                            let bound_label = if boundary.is_infinite() {
                                "+Inf".to_string()
                            } else {
                                boundary.to_string()
                            };
                            labels.push(("le".to_string(), bound_label));
                            write_sample(
                                &format!("{}_bucket", family.name),
                                &labels,
                                &cumulative.to_string(),
                                writer,
                            )?;
                        }
                        write_sample(
                            &format!("{}_sum", family.name),
                            &sample.labels,
                            &sum.to_string(),
                            writer,
                        )?;
                        write_sample(
                            &format!("{}_count", family.name),
                            &sample.labels,
                            &count.to_string(),
                            writer,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Counter {
    inner: Arc<CounterInner>,
}

struct CounterInner {
    name: String,
    help: String,
    labels: Vec<(String, String)>,
    value: AtomicU64,
}

impl Counter {
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(CounterInner {
                name: name.into(),
                help: help.into(),
                labels: Vec::new(),
                value: AtomicU64::new(0),
            }),
        })
    }

    pub fn inc(&self) {
        self.inc_by(1);
    }

    pub fn inc_by(&self, value: u64) {
        self.inner.value.fetch_add(value, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.inner.value.store(0, Ordering::Relaxed);
    }

    pub fn value(&self) -> u64 {
        self.inner.value.load(Ordering::Relaxed)
    }

    pub fn get(&self) -> u64 {
        self.value()
    }

    pub fn handle(&self) -> IntCounterHandle {
        IntCounterHandle::new(Ok(self.clone()))
    }
}

impl Collector for Counter {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Counter,
            samples: vec![MetricSample {
                labels: self.inner.labels.clone(),
                value: MetricSampleValue::Counter(self.value()),
            }],
        }
    }
}

pub type IntCounter = Counter;

#[derive(Clone)]
pub struct IntCounterHandle(Result<IntCounter>);

impl IntCounterHandle {
    fn new(inner: Result<IntCounter>) -> Self {
        Self(inner)
    }

    fn inner(&self) -> Option<&IntCounter> {
        self.0.as_ref().ok()
    }

    pub fn into_result(self) -> Result<IntCounter> {
        self.0
    }

    pub fn inc(&self) {
        if let Some(counter) = self.inner() {
            counter.inc();
        }
    }

    pub fn inc_by(&self, value: u64) {
        if let Some(counter) = self.inner() {
            counter.inc_by(value);
        }
    }

    pub fn reset(&self) {
        if let Some(counter) = self.inner() {
            counter.reset();
        }
    }

    pub fn value(&self) -> u64 {
        self.inner().map(|c| c.value()).unwrap_or(0)
    }

    pub fn get(&self) -> u64 {
        self.value()
    }
}

#[derive(Clone)]
pub struct CounterVec {
    inner: Arc<CounterVecInner>,
}

struct CounterVecInner {
    name: String,
    help: String,
    label_names: Arc<Vec<String>>,
    values: DashMap<LabelKey, Arc<CounterInner>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LabelKey(Vec<String>);

impl CounterVec {
    pub fn new(opts: Opts, label_names: &[&str]) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(CounterVecInner {
                name: opts.name,
                help: opts.help,
                label_names: Arc::new(label_names.iter().map(|s| (*s).to_string()).collect()),
                values: DashMap::new(),
            }),
        })
    }

    pub fn with_label_values(&self, values: &[&str]) -> IntCounterHandle {
        IntCounterHandle::new(self.try_with_label_values(values))
    }

    pub fn handle_for_label_values(&self, values: &[&str]) -> Result<IntCounterHandle> {
        self.get_metric_with_label_values(values)
            .map(|counter| counter.handle())
    }

    /// Returns a handle for the provided label values, registering the set if
    /// it has not been seen before.
    pub fn ensure_handle_for_label_values(&self, values: &[&str]) -> Result<IntCounterHandle> {
        match self.handle_for_label_values(values) {
            Ok(handle) => Ok(handle),
            Err(MetricError::MissingLabelSet) => {
                self.try_with_label_values(values)?;
                self.handle_for_label_values(values)
            }
            Err(err) => Err(err),
        }
    }

    pub fn try_with_label_values(&self, values: &[&str]) -> Result<IntCounter> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key_values: Vec<String> = values.iter().map(|s| (*s).to_string()).collect();
        let key = LabelKey(key_values.clone());
        if let Some(existing) = self.inner.values.get(&key) {
            return Ok(IntCounter {
                inner: existing.clone(),
            });
        }
        let labels = self
            .inner
            .label_names
            .iter()
            .cloned()
            .zip(key_values.into_iter())
            .collect::<Vec<_>>();
        let counter = Counter {
            inner: Arc::new(CounterInner {
                name: self.inner.name.clone(),
                help: self.inner.help.clone(),
                labels: labels.clone(),
                value: AtomicU64::new(0),
            }),
        };
        self.inner.values.insert(key, counter.inner.clone());
        Ok(counter)
    }

    pub fn remove_label_values(&self, values: &[&str]) -> bool {
        if values.len() != self.inner.label_names.len() {
            return false;
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner.values.remove(&key).is_some()
    }

    pub fn get_metric_with_label_values(&self, values: &[&str]) -> Result<IntCounter> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner
            .values
            .get(&key)
            .map(|inner| IntCounter {
                inner: inner.clone(),
            })
            .ok_or(MetricError::MissingLabelSet)
    }

    pub fn reset(&self) {
        for counter in self.inner.values.values() {
            counter.value.store(0, Ordering::Relaxed);
        }
    }
}

impl Collector for CounterVec {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        let mut samples = Vec::new();
        for inner in self.inner.values.values() {
            samples.push(MetricSample {
                labels: inner.labels.clone(),
                value: MetricSampleValue::Counter(inner.value.load(Ordering::Relaxed)),
            });
        }
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Counter,
            samples,
        }
    }
}

pub type IntCounterVec = CounterVec;

#[derive(Clone)]
pub struct GaugeHandle(Result<Gauge>);

impl GaugeHandle {
    fn new(inner: Result<Gauge>) -> Self {
        Self(inner)
    }

    fn inner(&self) -> Option<&Gauge> {
        self.0.as_ref().ok()
    }

    pub fn into_result(self) -> Result<Gauge> {
        self.0
    }

    pub fn set(&self, value: f64) {
        if let Some(gauge) = self.inner() {
            gauge.set(value);
        }
    }

    pub fn reset(&self) {
        if let Some(gauge) = self.inner() {
            gauge.reset();
        }
    }

    pub fn get(&self) -> f64 {
        self.inner().map(|g| g.get()).unwrap_or(0.0)
    }
}

#[derive(Clone)]
pub struct Gauge {
    inner: Arc<GaugeInner>,
}

struct GaugeInner {
    name: String,
    help: String,
    labels: Vec<(String, String)>,
    value: AtomicU64,
}

impl Gauge {
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(GaugeInner {
                name: name.into(),
                help: help.into(),
                labels: Vec::new(),
                value: AtomicU64::new(0f64.to_bits()),
            }),
        }
    }

    pub fn set(&self, value: f64) {
        self.inner.value.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.set(0.0);
    }

    pub fn get(&self) -> f64 {
        f64::from_bits(self.inner.value.load(Ordering::Relaxed))
    }

    pub fn handle(&self) -> GaugeHandle {
        GaugeHandle::new(Ok(self.clone()))
    }
}

impl Collector for Gauge {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Gauge,
            samples: vec![MetricSample {
                labels: self.inner.labels.clone(),
                value: MetricSampleValue::Gauge(self.get()),
            }],
        }
    }
}

#[derive(Clone)]
pub struct IntGauge {
    inner: Arc<IntGaugeInner>,
}

struct IntGaugeInner {
    name: String,
    help: String,
    labels: Vec<(String, String)>,
    value: AtomicI64,
}

impl IntGauge {
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(IntGaugeInner {
                name: name.into(),
                help: help.into(),
                labels: Vec::new(),
                value: AtomicI64::new(0),
            }),
        })
    }

    pub fn set(&self, value: i64) {
        self.inner.value.store(value, Ordering::Relaxed);
    }

    pub fn add(&self, value: i64) {
        self.inner.value.fetch_add(value, Ordering::Relaxed);
    }

    pub fn sub(&self, value: i64) {
        self.inner.value.fetch_sub(value, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.set(0);
    }

    pub fn value(&self) -> i64 {
        self.inner.value.load(Ordering::Relaxed)
    }

    pub fn get(&self) -> i64 {
        self.value()
    }

    pub fn handle(&self) -> IntGaugeHandle {
        IntGaugeHandle::new(Ok(self.clone()))
    }
}

impl Collector for IntGauge {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Gauge,
            samples: vec![MetricSample {
                labels: self.inner.labels.clone(),
                value: MetricSampleValue::IntGauge(self.value()),
            }],
        }
    }
}

#[derive(Clone)]
pub struct IntGaugeHandle(Result<IntGauge>);

impl IntGaugeHandle {
    fn new(inner: Result<IntGauge>) -> Self {
        Self(inner)
    }

    fn inner(&self) -> Option<&IntGauge> {
        self.0.as_ref().ok()
    }

    pub fn into_result(self) -> Result<IntGauge> {
        self.0
    }

    pub fn set(&self, value: i64) {
        if let Some(gauge) = self.inner() {
            gauge.set(value);
        }
    }

    pub fn add(&self, value: i64) {
        if let Some(gauge) = self.inner() {
            gauge.add(value);
        }
    }

    pub fn sub(&self, value: i64) {
        if let Some(gauge) = self.inner() {
            gauge.sub(value);
        }
    }

    pub fn reset(&self) {
        if let Some(gauge) = self.inner() {
            gauge.reset();
        }
    }

    pub fn value(&self) -> i64 {
        self.inner().map(|g| g.value()).unwrap_or(0)
    }

    pub fn get(&self) -> i64 {
        self.value()
    }
}

#[derive(Clone)]
pub struct IntGaugeVec {
    inner: Arc<IntGaugeVecInner>,
}

struct IntGaugeVecInner {
    name: String,
    help: String,
    label_names: Arc<Vec<String>>,
    values: DashMap<LabelKey, Arc<IntGaugeInner>>,
}

impl IntGaugeVec {
    pub fn new(opts: Opts, label_names: &[&str]) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(IntGaugeVecInner {
                name: opts.name,
                help: opts.help,
                label_names: Arc::new(label_names.iter().map(|s| (*s).to_string()).collect()),
                values: DashMap::new(),
            }),
        })
    }

    pub fn with_label_values(&self, values: &[&str]) -> IntGaugeHandle {
        IntGaugeHandle::new(self.try_with_label_values(values))
    }

    pub fn handle_for_label_values(&self, values: &[&str]) -> Result<IntGaugeHandle> {
        self.get_metric_with_label_values(values)
            .map(|gauge| gauge.handle())
    }

    /// Returns a handle for the provided label values, registering the set if
    /// it has not been seen before.
    pub fn ensure_handle_for_label_values(&self, values: &[&str]) -> Result<IntGaugeHandle> {
        match self.handle_for_label_values(values) {
            Ok(handle) => Ok(handle),
            Err(MetricError::MissingLabelSet) => {
                self.try_with_label_values(values)?;
                self.handle_for_label_values(values)
            }
            Err(err) => Err(err),
        }
    }

    pub fn try_with_label_values(&self, values: &[&str]) -> Result<IntGauge> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key_vec: Vec<String> = values.iter().map(|s| (*s).to_string()).collect();
        let key = LabelKey(key_vec.clone());
        if let Some(existing) = self.inner.values.get(&key) {
            return Ok(IntGauge {
                inner: existing.clone(),
            });
        }
        let labels = self
            .inner
            .label_names
            .iter()
            .cloned()
            .zip(key_vec.into_iter())
            .collect::<Vec<_>>();
        let gauge = IntGauge {
            inner: Arc::new(IntGaugeInner {
                name: self.inner.name.clone(),
                help: self.inner.help.clone(),
                labels: labels.clone(),
                value: AtomicI64::new(0),
            }),
        };
        self.inner.values.insert(key, gauge.inner.clone());
        Ok(gauge)
    }

    pub fn remove_label_values(&self, values: &[&str]) -> bool {
        if values.len() != self.inner.label_names.len() {
            return false;
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner.values.remove(&key).is_some()
    }

    pub fn get_metric_with_label_values(&self, values: &[&str]) -> Result<IntGauge> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner
            .values
            .get(&key)
            .map(|inner| IntGauge {
                inner: inner.clone(),
            })
            .ok_or(MetricError::MissingLabelSet)
    }

    pub fn reset(&self) {
        for gauge in self.inner.values.values() {
            gauge.value.store(0, Ordering::Relaxed);
        }
    }
}

impl Collector for IntGaugeVec {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        let mut samples = Vec::new();
        for inner in self.inner.values.values() {
            samples.push(MetricSample {
                labels: inner.labels.clone(),
                value: MetricSampleValue::IntGauge(inner.value.load(Ordering::Relaxed)),
            });
        }
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Gauge,
            samples,
        }
    }
}

#[derive(Clone)]
pub struct GaugeVec {
    inner: Arc<GaugeVecInner>,
}

struct GaugeVecInner {
    name: String,
    help: String,
    label_names: Arc<Vec<String>>,
    values: DashMap<LabelKey, Arc<GaugeInner>>,
}

impl GaugeVec {
    pub fn new(opts: Opts, label_names: &[&str]) -> Self {
        Self {
            inner: Arc::new(GaugeVecInner {
                name: opts.name,
                help: opts.help,
                label_names: Arc::new(label_names.iter().map(|s| (*s).to_string()).collect()),
                values: DashMap::new(),
            }),
        }
    }

    pub fn with_label_values(&self, values: &[&str]) -> GaugeHandle {
        GaugeHandle::new(self.try_with_label_values(values))
    }

    pub fn handle_for_label_values(&self, values: &[&str]) -> Result<GaugeHandle> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner
            .values
            .get(&key)
            .map(|inner| {
                Gauge {
                    inner: inner.clone(),
                }
                .handle()
            })
            .ok_or(MetricError::MissingLabelSet)
    }

    /// Returns a handle for the provided label values, registering the set if
    /// it has not been seen before.
    pub fn ensure_handle_for_label_values(&self, values: &[&str]) -> Result<GaugeHandle> {
        match self.handle_for_label_values(values) {
            Ok(handle) => Ok(handle),
            Err(MetricError::MissingLabelSet) => {
                self.try_with_label_values(values)?;
                self.handle_for_label_values(values)
            }
            Err(err) => Err(err),
        }
    }

    pub fn try_with_label_values(&self, values: &[&str]) -> Result<Gauge> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key_vec: Vec<String> = values.iter().map(|s| (*s).to_string()).collect();
        let key = LabelKey(key_vec.clone());
        if let Some(existing) = self.inner.values.get(&key) {
            return Ok(Gauge {
                inner: existing.clone(),
            });
        }
        let labels = self
            .inner
            .label_names
            .iter()
            .cloned()
            .zip(key_vec.into_iter())
            .collect::<Vec<_>>();
        let gauge = Gauge {
            inner: Arc::new(GaugeInner {
                name: self.inner.name.clone(),
                help: self.inner.help.clone(),
                labels: labels.clone(),
                value: AtomicU64::new(0f64.to_bits()),
            }),
        };
        self.inner.values.insert(key, gauge.inner.clone());
        Ok(gauge)
    }

    pub fn remove_label_values(&self, values: &[&str]) -> bool {
        if values.len() != self.inner.label_names.len() {
            return false;
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner.values.remove(&key).is_some()
    }

    pub fn reset(&self) {
        for gauge in self.inner.values.values() {
            gauge.value.store(0f64.to_bits(), Ordering::Relaxed);
        }
    }
}

impl Collector for GaugeVec {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        let mut samples = Vec::new();
        for inner in self.inner.values.values() {
            samples.push(MetricSample {
                labels: inner.labels.clone(),
                value: MetricSampleValue::Gauge(f64::from_bits(
                    inner.value.load(Ordering::Relaxed),
                )),
            });
        }
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Gauge,
            samples,
        }
    }
}

#[derive(Clone)]
pub struct Histogram {
    inner: Arc<HistogramInner>,
}

struct HistogramInner {
    name: String,
    help: String,
    labels: Vec<(String, String)>,
    config: Arc<HistogramConfig>,
    counts: Vec<AtomicU64>,
    sum_bits: AtomicU64,
    count: AtomicU64,
}

#[derive(Clone)]
struct HistogramConfig {
    buckets: Vec<f64>,
}

impl HistogramConfig {
    fn new(mut buckets: Vec<f64>) -> Result<Self> {
        if buckets.is_empty() {
            return Err(MetricError::InvalidBuckets("at least one bucket required"));
        }
        buckets.sort_by(|a, b| a.partial_cmp(b).unwrap());
        buckets.dedup();
        if buckets.is_empty() {
            return Err(MetricError::InvalidBuckets(
                "bucket deduplication removed all entries",
            ));
        }
        Ok(Self { buckets })
    }

    fn bucket_index(&self, value: f64) -> usize {
        for (idx, bound) in self.buckets.iter().enumerate() {
            if value <= *bound {
                return idx;
            }
        }
        self.buckets.len()
    }
}

#[derive(Clone)]
pub struct HistogramOpts {
    name: String,
    help: String,
    buckets: Vec<f64>,
}

impl HistogramOpts {
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
            buckets: vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        }
    }

    pub fn buckets(mut self, buckets: Vec<f64>) -> Self {
        self.buckets = buckets;
        self
    }
}

impl Histogram {
    pub fn with_opts(opts: HistogramOpts) -> Result<Self> {
        let config = HistogramConfig::new(opts.buckets)?;
        let len = config.buckets.len() + 1; // include +Inf bucket
        Ok(Self {
            inner: Arc::new(HistogramInner {
                name: opts.name,
                help: opts.help,
                labels: Vec::new(),
                config: Arc::new(config),
                counts: (0..len).map(|_| AtomicU64::new(0)).collect(),
                sum_bits: AtomicU64::new(0f64.to_bits()),
                count: AtomicU64::new(0),
            }),
        })
    }

    pub fn observe(&self, value: f64) {
        let idx = self.inner.config.bucket_index(value);
        self.inner.counts[idx].fetch_add(1, Ordering::Relaxed);
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        add_to_atomic_f64(&self.inner.sum_bits, value);
    }

    pub fn get_sample_count(&self) -> u64 {
        self.inner.count.load(Ordering::Relaxed)
    }

    fn with_labels(&self, labels: Vec<(String, String)>) -> Self {
        let len = self.inner.config.buckets.len() + 1;
        Self {
            inner: Arc::new(HistogramInner {
                name: self.inner.name.clone(),
                help: self.inner.help.clone(),
                labels,
                config: Arc::clone(&self.inner.config),
                counts: (0..len).map(|_| AtomicU64::new(0)).collect(),
                sum_bits: AtomicU64::new(0f64.to_bits()),
                count: AtomicU64::new(0),
            }),
        }
    }

    pub fn handle(&self) -> HistogramHandle {
        HistogramHandle::new(Ok(self.clone()))
    }
}

impl Collector for Histogram {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.help
    }

    fn collect(&self) -> MetricFamily {
        let mut buckets = Vec::new();
        for (idx, bound) in self.inner.config.buckets.iter().enumerate() {
            let count = self.inner.counts[idx].load(Ordering::Relaxed);
            buckets.push((*bound, count));
        }
        let inf_count = self
            .inner
            .counts
            .last()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0);
        buckets.push((f64::INFINITY, inf_count));
        MetricFamily {
            name: self.inner.name.clone(),
            help: self.inner.help.clone(),
            r#type: MetricType::Histogram,
            samples: vec![MetricSample {
                labels: self.inner.labels.clone(),
                value: MetricSampleValue::Histogram {
                    buckets,
                    count: self.inner.count.load(Ordering::Relaxed),
                    sum: f64::from_bits(self.inner.sum_bits.load(Ordering::Relaxed)),
                },
            }],
        }
    }
}

#[derive(Clone)]
pub struct HistogramHandle(Result<Histogram>);

impl HistogramHandle {
    fn new(inner: Result<Histogram>) -> Self {
        Self(inner)
    }

    fn inner(&self) -> Option<&Histogram> {
        self.0.as_ref().ok()
    }

    pub fn into_result(self) -> Result<Histogram> {
        self.0
    }

    pub fn observe(&self, value: f64) {
        if let Some(hist) = self.inner() {
            hist.observe(value);
        }
    }

    pub fn get_sample_count(&self) -> u64 {
        self.inner().map(|h| h.get_sample_count()).unwrap_or(0)
    }
}

#[derive(Clone)]
pub struct HistogramVec {
    inner: Arc<HistogramVecInner>,
}

struct HistogramVecInner {
    base: Histogram,
    label_names: Arc<Vec<String>>,
    values: DashMap<LabelKey, Arc<HistogramInner>>,
}

impl HistogramVec {
    pub fn new(opts: HistogramOpts, label_names: &[&str]) -> Result<Self> {
        let base = Histogram::with_opts(opts)?;
        Ok(Self {
            inner: Arc::new(HistogramVecInner {
                label_names: Arc::new(label_names.iter().map(|s| (*s).to_string()).collect()),
                base: base.clone(),
                values: DashMap::new(),
            }),
        })
    }

    pub fn with_label_values(&self, values: &[&str]) -> HistogramHandle {
        HistogramHandle::new(self.try_with_label_values(values))
    }

    pub fn handle_for_label_values(&self, values: &[&str]) -> Result<HistogramHandle> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner
            .values
            .get(&key)
            .map(|inner| {
                Histogram {
                    inner: inner.clone(),
                }
                .handle()
            })
            .ok_or(MetricError::MissingLabelSet)
    }

    /// Returns a handle for the provided label values, registering the set if
    /// it has not been seen before.
    pub fn ensure_handle_for_label_values(&self, values: &[&str]) -> Result<HistogramHandle> {
        match self.handle_for_label_values(values) {
            Ok(handle) => Ok(handle),
            Err(MetricError::MissingLabelSet) => {
                self.try_with_label_values(values)?;
                self.handle_for_label_values(values)
            }
            Err(err) => Err(err),
        }
    }

    pub fn try_with_label_values(&self, values: &[&str]) -> Result<Histogram> {
        if values.len() != self.inner.label_names.len() {
            return Err(MetricError::InconsistentCardinality {
                expected: self.inner.label_names.len(),
                actual: values.len(),
            });
        }
        let key_vec: Vec<String> = values.iter().map(|s| (*s).to_string()).collect();
        let key = LabelKey(key_vec.clone());
        if let Some(existing) = self.inner.values.get(&key) {
            return Ok(Histogram {
                inner: existing.clone(),
            });
        }
        let labels = self
            .inner
            .label_names
            .iter()
            .cloned()
            .zip(key_vec.into_iter())
            .collect::<Vec<_>>();
        let hist = self.inner.base.with_labels(labels);
        self.inner.values.insert(key, hist.inner.clone());
        Ok(hist)
    }

    pub fn remove_label_values(&self, values: &[&str]) -> bool {
        if values.len() != self.inner.label_names.len() {
            return false;
        }
        let key = LabelKey(values.iter().map(|s| (*s).to_string()).collect());
        self.inner.values.remove(&key).is_some()
    }
}

impl Collector for HistogramVec {
    fn name(&self) -> &str {
        &self.inner.base.inner.name
    }

    fn help(&self) -> &str {
        &self.inner.base.inner.help
    }

    fn collect(&self) -> MetricFamily {
        let mut samples = Vec::new();
        for inner in self.inner.values.values() {
            let mut buckets = Vec::new();
            for (idx, bound) in self.inner.base.inner.config.buckets.iter().enumerate() {
                let count = inner.counts[idx].load(Ordering::Relaxed);
                buckets.push((*bound, count));
            }
            let inf_count = inner
                .counts
                .last()
                .map(|c| c.load(Ordering::Relaxed))
                .unwrap_or(0);
            buckets.push((f64::INFINITY, inf_count));
            samples.push(MetricSample {
                labels: inner.labels.clone(),
                value: MetricSampleValue::Histogram {
                    buckets,
                    count: inner.count.load(Ordering::Relaxed),
                    sum: f64::from_bits(inner.sum_bits.load(Ordering::Relaxed)),
                },
            });
        }
        MetricFamily {
            name: self.inner.base.inner.name.clone(),
            help: self.inner.base.inner.help.clone(),
            r#type: MetricType::Histogram,
            samples,
        }
    }
}

fn add_to_atomic_f64(target: &AtomicU64, delta: f64) {
    let mut current = target.load(Ordering::Relaxed);
    loop {
        let value = f64::from_bits(current);
        let new_bits = (value + delta).to_bits();
        match target.compare_exchange(current, new_bits, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

#[derive(Clone, Default)]
pub struct Opts {
    name: String,
    help: String,
}

impl Opts {
    pub fn new(name: &str, help: &str) -> Self {
        Self {
            name: name.to_string(),
            help: help.to_string(),
        }
    }

    pub fn namespace(mut self, namespace: &str) -> Self {
        self.name = format!("{}_{}", namespace, self.name);
        self
    }
}

pub fn exponential_buckets(start: f64, factor: f64, count: usize) -> Vec<f64> {
    let mut buckets = Vec::with_capacity(count);
    let mut current = start;
    for _ in 0..count {
        buckets.push(current);
        current *= factor;
    }
    buckets
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
        let gauge = Gauge::new("temperature_celsius", "Temperature");
        registry
            .register(Box::new(gauge.clone()))
            .expect("register gauge");
        gauge.set(42.5);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].name, "temperature_celsius");
        assert_eq!(snapshot[0].r#type, MetricType::Gauge);
    }

    #[test]
    fn duplicate_names_error() {
        let registry = Registry::new();
        registry
            .register_counter("dup_total", "first")
            .expect("first registration succeeds");
        let err = match registry.register_counter("dup_total", "second") {
            Ok(_) => panic!("duplicate registration succeeds unexpectedly"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            TelemetryError::DuplicateMetric("dup_total".to_string())
        );
    }

    #[test]
    fn counter_handle_ignores_cardinality_errors() {
        let vec = CounterVec::new(Opts::new("sample_total", "Sample counter"), &["label"]).unwrap();
        let invalid = vec
            .ensure_handle_for_label_values(&[])
            .expect(LABEL_REGISTRATION_ERR);
        invalid.inc();
        invalid.inc_by(5);
        assert!(matches!(
            vec.handle_for_label_values(&["ok"]),
            Err(MetricError::MissingLabelSet)
        ));

        let valid = vec
            .ensure_handle_for_label_values(&["ok"])
            .expect(LABEL_REGISTRATION_ERR);
        valid.inc();
        assert_eq!(valid.get(), 1);
    }

    #[test]
    fn int_gauge_handle_ignores_cardinality_errors() {
        let vec = IntGaugeVec::new(Opts::new("sample_gauge", "Sample gauge"), &["label"]).unwrap();
        let invalid = vec
            .ensure_handle_for_label_values(&[])
            .expect(LABEL_REGISTRATION_ERR);
        invalid.set(10);
        invalid.add(5);
        invalid.sub(3);
        assert!(matches!(
            vec.handle_for_label_values(&["present"]),
            Err(MetricError::MissingLabelSet)
        ));

        let valid = vec
            .ensure_handle_for_label_values(&["present"])
            .expect(LABEL_REGISTRATION_ERR);
        valid.set(10);
        valid.add(5);
        valid.sub(3);
        assert_eq!(valid.value(), 12);
    }

    #[test]
    fn histogram_handle_ignores_cardinality_errors() {
        let vec = HistogramVec::new(HistogramOpts::new("latency_seconds", "Latency"), &["label"])
            .unwrap();
        let invalid = vec
            .ensure_handle_for_label_values(&[])
            .expect(LABEL_REGISTRATION_ERR);
        invalid.observe(1.0);
        let valid = vec
            .ensure_handle_for_label_values(&["ok"])
            .expect(LABEL_REGISTRATION_ERR);
        assert_eq!(valid.get_sample_count(), 0);
        valid.observe(1.0);
        assert_eq!(valid.get_sample_count(), 1);
    }
}
