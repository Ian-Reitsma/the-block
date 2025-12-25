#![allow(
    clippy::needless_lifetimes,
    clippy::explicit_auto_deref,
    clippy::unwrap_or_default,
    clippy::type_complexity,
    clippy::redundant_closure
)]
#![forbid(unsafe_code)]

//! First-party test harness primitives that replace the previous dependency on
//! external benchmarking, property-testing, and snapshot crates. The goal is
//! to provide pragmatic, deterministic tooling that keeps the workspace free
//! from third-party test harnesses while still exercising meaningful coverage.

/// Benchmark helpers that execute the provided closure a fixed number of
/// iterations and emit human-readable timing summaries.
pub mod bench {
    use std::collections::HashMap;
    use std::env;
    use std::fs::{self, File};
    use std::io::{ErrorKind, Read};
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const PERCENTILES: [(&str, f64); 3] = [("p50", 0.50), ("p90", 0.90), ("p99", 0.99)];
    const SUPPORTED_THRESHOLD_KEYS: [&str; 4] = ["per_iter", "p50", "p90", "p99"];

    fn is_supported_threshold(key: &str) -> bool {
        SUPPORTED_THRESHOLD_KEYS
            .iter()
            .any(|candidate| candidate == &key)
    }

    #[derive(Clone, Copy, Debug)]
    struct PercentileSample {
        label: &'static str,
        value: Duration,
    }

    #[derive(Clone, Copy, Debug)]
    enum ComparisonKind {
        PerIteration,
        Percentile(&'static str),
    }

    impl ComparisonKind {
        fn key(&self) -> &'static str {
            match self {
                ComparisonKind::PerIteration => "per_iter",
                ComparisonKind::Percentile(label) => label,
            }
        }

        fn metric_name(&self, base: &str) -> String {
            match self {
                ComparisonKind::PerIteration => base.to_string(),
                ComparisonKind::Percentile(label) => format!("{base}_{label}"),
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct MetricComparison {
        kind: ComparisonKind,
        value: f64,
        threshold: Option<f64>,
        exceeded: bool,
    }

    impl MetricComparison {
        fn new(kind: ComparisonKind, value: Option<f64>, threshold: Option<f64>) -> Self {
            let (value, exceeded) = match (value, threshold) {
                (Some(v), Some(limit)) => (v, v > limit),
                (Some(v), None) => (v, false),
                (None, Some(_limit)) => (f64::NAN, true),
                (None, None) => (f64::NAN, false),
            };
            Self {
                kind,
                value,
                threshold,
                exceeded,
            }
        }

        fn describe(&self) -> String {
            match (self.value.is_nan(), self.threshold) {
                (true, Some(limit)) => {
                    format!("{} missing (threshold {:.6})", self.kind.key(), limit)
                }
                (false, Some(limit)) => {
                    format!("{} {:.6} > {:.6}", self.kind.key(), self.value, limit)
                }
                _ => self.kind.key().to_string(),
            }
        }
    }

    #[derive(Clone, Debug)]
    struct RegressionReport {
        comparisons: Vec<MetricComparison>,
    }

    impl RegressionReport {
        fn triggered(&self) -> bool {
            self.comparisons.iter().any(|cmp| cmp.exceeded)
        }

        fn exceeded(&self) -> impl Iterator<Item = &MetricComparison> {
            self.comparisons.iter().filter(|cmp| cmp.exceeded)
        }

        fn comparisons(&self) -> &[MetricComparison] {
            &self.comparisons
        }
    }

    struct BenchmarkSnapshot<'a> {
        name: &'a str,
        iterations: usize,
        elapsed: Duration,
        per_iteration: Duration,
        percentiles: Vec<PercentileSample>,
    }

    impl<'a> BenchmarkSnapshot<'a> {
        fn percentile_value(&self, label: &str) -> Option<Duration> {
            self.percentiles
                .iter()
                .find(|sample| sample.label == label)
                .map(|sample| sample.value)
        }

        fn percentile_or(&self, label: &str, default: Duration) -> Duration {
            self.percentile_value(label).unwrap_or(default)
        }
    }

    /// Default number of iterations used when the benchmark macro does not
    /// specify a custom count.
    pub const DEFAULT_ITERATIONS: usize = 100;

    /// Result of a benchmark run.
    #[derive(Debug, Clone)]
    pub struct BenchResult {
        /// Number of iterations executed.
        pub iterations: usize,
        /// Total elapsed wall-clock time.
        pub elapsed: Duration,
        pub samples: Vec<Duration>,
    }

    impl BenchResult {
        /// Average duration per iteration.
        pub fn per_iteration(&self) -> Duration {
            if self.iterations == 0 {
                return Duration::from_secs(0);
            }
            self.elapsed / self.iterations as u32
        }

        /// Returns the recorded per-iteration samples sorted in ascending order.
        pub fn samples(&self) -> &[Duration] {
            &self.samples
        }

        /// Returns the duration at the requested percentile using nearest-rank rounding.
        pub fn percentile(&self, quantile: f64) -> Option<Duration> {
            if self.samples.is_empty() {
                return None;
            }
            let clamped = quantile.clamp(0.0, 1.0);
            let idx = ((self.samples.len() - 1) as f64 * clamped).round() as usize;
            self.samples.get(idx).copied()
        }
    }

    /// Runs a benchmark by executing `body` `iterations` times.
    pub fn run<F>(name: &str, iterations: usize, mut body: F)
    where
        F: FnMut(),
    {
        let iterations = iterations.max(1);
        let mut samples = Vec::with_capacity(iterations);
        let mut total = Duration::from_secs(0);
        for _ in 0..iterations {
            let started = Instant::now();
            body();
            let elapsed = started.elapsed();
            total += elapsed;
            samples.push(elapsed);
        }
        samples.sort();
        record_result(
            name,
            BenchResult {
                iterations,
                elapsed: total,
                samples,
            },
        );
    }

    /// Record a pre-computed benchmark [`BenchResult`]. This is primarily used by
    /// integration tests that need to exercise history persistence without
    /// executing the benchmark body.
    pub fn record_result(name: &str, result: BenchResult) {
        report(name, result);
    }

    fn report(name: &str, result: BenchResult) {
        let snapshot = BenchmarkSnapshot {
            name,
            iterations: result.iterations,
            elapsed: result.elapsed,
            per_iteration: result.per_iteration(),
            percentiles: PERCENTILES
                .iter()
                .filter_map(|(label, quantile)| {
                    result.percentile(*quantile).map(|value| PercentileSample {
                        label: *label,
                        value,
                    })
                })
                .collect(),
        };
        let regression = evaluate_thresholds(&snapshot);
        let p50 = snapshot.percentile_or("p50", snapshot.per_iteration);
        let p90 = snapshot.percentile_or("p90", snapshot.per_iteration);
        let p99 = snapshot.percentile_or("p99", snapshot.per_iteration);
        println!(
            "benchmark `{name}`: {iters} iters in {total:?} ({avg:?}/iter, p50={p50:?}, p90={p90:?}, p99={p99:?})",
            iters = snapshot.iterations,
            total = snapshot.elapsed,
            avg = snapshot.per_iteration,
            p50 = p50,
            p90 = p90,
            p99 = p99
        );
        if regression.triggered() {
            let details: Vec<String> = regression.exceeded().map(|cmp| cmp.describe()).collect();
            eprintln!(
                "benchmark `{name}` regression detected: {}",
                details.join(", ")
            );
        }
        if let Err(err) = export_prometheus(&snapshot, &regression) {
            eprintln!("failed to export benchmark metric: {err}");
        }
        if let Err(err) = persist_history(&snapshot, &regression) {
            eprintln!("failed to persist benchmark history: {err}");
        }
        if let Err(err) = emit_alert(&snapshot, &regression) {
            eprintln!("failed to emit benchmark alert: {err}");
        }
    }

    fn export_prometheus(
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
    ) -> Result<(), std::io::Error> {
        let path = match env::var("TB_BENCH_PROM_PATH") {
            Ok(value) if !value.is_empty() => value,
            _ => return Ok(()),
        };
        let sanitized = sanitize_metric_name(snapshot.name);
        let metric_name = if sanitized.is_empty() {
            "benchmark_seconds".to_string()
        } else {
            format!("benchmark_{}_seconds", sanitized)
        };
        let metrics = build_metric_series(&metric_name, snapshot, regression);
        let path_buf = PathBuf::from(path);
        with_file_lock(&path_buf, || export_prometheus_locked(&path_buf, &metrics))
    }

    fn build_metric_series(
        metric_name: &str,
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
    ) -> Vec<(String, f64)> {
        let mut metrics = Vec::new();
        metrics.push((
            metric_name.to_string(),
            snapshot.per_iteration.as_secs_f64(),
        ));
        metrics.push((
            format!("{metric_name}_iterations"),
            snapshot.iterations as f64,
        ));
        for sample in &snapshot.percentiles {
            metrics.push((
                format!("{metric_name}_{}", sample.label),
                sample.value.as_secs_f64(),
            ));
        }
        metrics.push((
            format!("{metric_name}_regression"),
            if regression.triggered() { 1.0 } else { 0.0 },
        ));
        for comparison in regression.comparisons() {
            if let Some(threshold) = comparison.threshold {
                let metric = comparison.kind.metric_name(metric_name);
                metrics.push((format!("{metric}_threshold"), threshold));
                metrics.push((
                    format!("{metric}_regression"),
                    if comparison.exceeded { 1.0 } else { 0.0 },
                ));
            }
        }
        metrics
    }

    const HISTORY_HEADER: &str = "timestamp,iterations,elapsed_seconds,per_iter_seconds,p50_seconds,p90_seconds,p99_seconds,regressions,per_iter_ewma_seconds,p50_ewma_seconds,p90_ewma_seconds,p99_ewma_seconds";
    const EWMA_ALPHA: f64 = 0.2;

    #[derive(Clone, Copy)]
    struct HistoryEwma {
        per_iter: f64,
        p50: Option<f64>,
        p90: Option<f64>,
        p99: Option<f64>,
    }

    fn persist_history(
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
    ) -> Result<(), std::io::Error> {
        let path = match env::var("TB_BENCH_HISTORY_PATH") {
            Ok(value) if !value.is_empty() => PathBuf::from(value),
            _ => return Ok(()),
        };
        let limit = env::var("TB_BENCH_HISTORY_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        with_file_lock(&path, || {
            persist_history_locked(&path, snapshot, regression, limit)
        })
    }

    fn build_history_line(
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
        ewma: &HistoryEwma,
    ) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let p50 = snapshot.percentile_value("p50");
        let p90 = snapshot.percentile_value("p90");
        let p99 = snapshot.percentile_value("p99");
        let regressions: Vec<&'static str> =
            regression.exceeded().map(|cmp| cmp.kind.key()).collect();
        let per_iter_ewma = format!("{:.9}", ewma.per_iter);
        let p50_ewma = format_optional_seconds(ewma.p50);
        let p90_ewma = format_optional_seconds(ewma.p90);
        let p99_ewma = format_optional_seconds(ewma.p99);
        let regressions = if regressions.is_empty() {
            "none".to_string()
        } else {
            regressions.join("|")
        };
        format!(
            "{timestamp:.6},{iters},{elapsed:.9},{per_iter:.9},{p50},{p90},{p99},{regressions},{per_iter_ewma},{p50_ewma},{p90_ewma},{p99_ewma}",
            iters = snapshot.iterations,
            elapsed = snapshot.elapsed.as_secs_f64(),
            per_iter = snapshot.per_iteration.as_secs_f64(),
            p50 = format_optional_duration(p50),
            p90 = format_optional_duration(p90),
            p99 = format_optional_duration(p99),
            regressions = regressions,
            per_iter_ewma = per_iter_ewma,
            p50_ewma = p50_ewma,
            p90_ewma = p90_ewma,
            p99_ewma = p99_ewma,
        )
    }

    fn format_optional_duration(value: Option<Duration>) -> String {
        value
            .map(|duration| format!("{:.9}", duration.as_secs_f64()))
            .unwrap_or_else(|| "".to_string())
    }

    fn format_optional_seconds(value: Option<f64>) -> String {
        value
            .map(|seconds| format!("{seconds:.9}"))
            .unwrap_or_else(|| "".to_string())
    }

    fn persist_history_locked(
        path: &Path,
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
        limit: Option<usize>,
    ) -> Result<(), std::io::Error> {
        let mut existing = String::new();
        if let Ok(mut file) = File::open(path) {
            file.read_to_string(&mut existing)?;
        }
        let mut rows: Vec<String> = existing
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("timestamp") {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect();
        let previous_ewma = parse_previous_ewma(&rows);
        let ewma = compute_history_ewma(snapshot, previous_ewma);
        let line = build_history_line(snapshot, regression, &ewma);
        rows.push(line);
        if let Some(limit) = limit {
            if limit == 0 {
                rows.clear();
            } else if rows.len() > limit {
                let start = rows.len() - limit;
                rows = rows.split_off(start);
            }
        }
        let mut output = String::with_capacity((rows.len() + 1) * HISTORY_HEADER.len());
        output.push_str(HISTORY_HEADER);
        output.push('\n');
        for row in &rows {
            output.push_str(row);
            output.push('\n');
        }
        fs::write(path, output)
    }

    fn parse_previous_ewma(rows: &[String]) -> Option<HistoryEwma> {
        rows.last().and_then(|line| {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 12 {
                let per_iter = parts[8].trim().parse::<f64>().ok()?;
                let p50 = parse_optional_seconds(parts[9]);
                let p90 = parse_optional_seconds(parts[10]);
                let p99 = parse_optional_seconds(parts[11]);
                Some(HistoryEwma {
                    per_iter,
                    p50,
                    p90,
                    p99,
                })
            } else if parts.len() >= 8 {
                let per_iter = parts[3].trim().parse::<f64>().ok()?;
                let p50 = parse_optional_seconds(parts[4]);
                let p90 = parse_optional_seconds(parts[5]);
                let p99 = parse_optional_seconds(parts[6]);
                Some(HistoryEwma {
                    per_iter,
                    p50,
                    p90,
                    p99,
                })
            } else {
                None
            }
        })
    }

    fn parse_optional_seconds(value: &str) -> Option<f64> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<f64>().ok()
        }
    }

    fn compute_history_ewma(
        snapshot: &BenchmarkSnapshot<'_>,
        previous: Option<HistoryEwma>,
    ) -> HistoryEwma {
        let per_iter = snapshot.per_iteration.as_secs_f64();
        let p50 = snapshot.percentile_value("p50").map(|d| d.as_secs_f64());
        let p90 = snapshot.percentile_value("p90").map(|d| d.as_secs_f64());
        let p99 = snapshot.percentile_value("p99").map(|d| d.as_secs_f64());
        if let Some(prev) = previous {
            HistoryEwma {
                per_iter: ewma_value(per_iter, prev.per_iter),
                p50: ewma_optional(p50, prev.p50),
                p90: ewma_optional(p90, prev.p90),
                p99: ewma_optional(p99, prev.p99),
            }
        } else {
            HistoryEwma {
                per_iter,
                p50,
                p90,
                p99,
            }
        }
    }

    fn ewma_value(current: f64, previous: f64) -> f64 {
        (EWMA_ALPHA * current) + ((1.0 - EWMA_ALPHA) * previous)
    }

    fn ewma_optional(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
        match (current, previous) {
            (Some(curr), Some(prev)) => Some(ewma_value(curr, prev)),
            (Some(curr), None) => Some(curr),
            (None, Some(prev)) => Some(prev),
            (None, None) => None,
        }
    }

    fn emit_alert(
        snapshot: &BenchmarkSnapshot<'_>,
        regression: &RegressionReport,
    ) -> Result<(), std::io::Error> {
        if !regression.triggered() {
            return Ok(());
        }
        let message = {
            let details: Vec<String> = regression.exceeded().map(|cmp| cmp.describe()).collect();
            format!(
                "benchmark `{}` regression: {}",
                snapshot.name,
                details.join(", ")
            )
        };
        if let Ok(path) = env::var("TB_BENCH_ALERT_PATH") {
            if !path.is_empty() {
                let path_buf = PathBuf::from(path);
                with_file_lock(&path_buf, || fs::write(&path_buf, &message))?;
            }
        }
        Ok(())
    }

    fn evaluate_thresholds(snapshot: &BenchmarkSnapshot<'_>) -> RegressionReport {
        let mut thresholds = load_thresholds_from_config(snapshot.name);
        for (key, value) in parse_thresholds_env() {
            thresholds.insert(key, value);
        }
        let mut comparisons = Vec::new();
        let per_iter = snapshot.per_iteration.as_secs_f64();
        comparisons.push(MetricComparison::new(
            ComparisonKind::PerIteration,
            Some(per_iter),
            thresholds.remove("per_iter"),
        ));
        let percentile_map: HashMap<&str, f64> = snapshot
            .percentiles
            .iter()
            .map(|sample| (sample.label, sample.value.as_secs_f64()))
            .collect();
        for (label, _quantile) in PERCENTILES.iter() {
            let value = percentile_map.get(label).copied();
            let threshold = thresholds.remove(*label);
            comparisons.push(MetricComparison::new(
                ComparisonKind::Percentile(label),
                value,
                threshold,
            ));
        }
        if !thresholds.is_empty() {
            let mut keys: Vec<_> = thresholds.keys().cloned().collect();
            keys.sort();
            panic!(
                "benchmark `{}` threshold configuration contains unknown keys: {} (supported keys: {})",
                snapshot.name,
                keys.join(", "),
                SUPPORTED_THRESHOLD_KEYS.join(", ")
            );
        }
        RegressionReport { comparisons }
    }

    fn parse_thresholds_env() -> HashMap<String, f64> {
        match env::var("TB_BENCH_REGRESSION_THRESHOLDS") {
            Ok(raw) if !raw.trim().is_empty() => {
                let mut map = HashMap::new();
                for entry in raw.split(',') {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let mut parts = trimmed.splitn(2, '=');
                    let key = parts.next().unwrap().trim().to_lowercase();
                    let Some(value_str) = parts.next() else {
                        continue;
                    };
                    if let Ok(value) = value_str.trim().parse::<f64>() {
                        if !is_supported_threshold(&key) {
                            eprintln!(
                                "ignoring unsupported benchmark threshold `{key}` from TB_BENCH_REGRESSION_THRESHOLDS"
                            );
                            continue;
                        }
                        map.insert(key, value);
                    }
                }
                map
            }
            _ => HashMap::new(),
        }
    }

    fn load_thresholds_from_config(name: &str) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        let sanitized = sanitize_metric_name(name);
        if sanitized.is_empty() {
            return map;
        }
        if let Some(path) = benchmark_threshold_path(&sanitized) {
            if let Ok(contents) = fs::read_to_string(&path) {
                for line in contents.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    let mut parts = trimmed.splitn(2, '=');
                    let key = parts.next().unwrap().trim().to_lowercase();
                    let Some(value_str) = parts.next() else {
                        continue;
                    };
                    if let Ok(value) = value_str.trim().parse::<f64>() {
                        if !is_supported_threshold(&key) {
                            eprintln!(
                                "ignoring unsupported benchmark threshold `{key}` from {}",
                                path.display()
                            );
                            continue;
                        }
                        map.insert(key, value);
                    }
                }
            }
        }
        map
    }

    fn benchmark_threshold_path(name: &str) -> Option<PathBuf> {
        if let Ok(dir) = env::var("TB_BENCH_THRESHOLD_DIR") {
            if !dir.trim().is_empty() {
                let path = PathBuf::from(dir).join(format!("{name}.thresholds"));
                if path.exists() {
                    return Some(path);
                }
            }
        }
        default_threshold_dir().and_then(|dir| {
            let path = dir.join(format!("{name}.thresholds"));
            if path.exists() {
                Some(path)
            } else {
                None
            }
        })
    }

    fn default_threshold_dir() -> Option<PathBuf> {
        let mut dir = env::current_dir().ok()?;
        for _ in 0..5 {
            let candidate = dir.join("config").join("benchmarks");
            if candidate.is_dir() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
        None
    }

    fn sanitize_metric_name(name: &str) -> String {
        name.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn with_file_lock<F>(path: &Path, mut body: F) -> Result<(), std::io::Error>
    where
        F: FnMut() -> Result<(), std::io::Error>,
    {
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match File::options()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(lock) => {
                    let result = body();
                    drop(lock);
                    let _ = fs::remove_file(&lock_path);
                    return result;
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            format!(
                                "timed out acquiring benchmark export lock {}",
                                lock_path.display()
                            ),
                        ));
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn export_prometheus_locked(
        path: &Path,
        metrics: &[(String, f64)],
    ) -> Result<(), std::io::Error> {
        let mut existing = String::new();
        if let Ok(mut file) = File::open(path) {
            file.read_to_string(&mut existing)?;
        }
        let metric_names: Vec<&str> = metrics.iter().map(|(name, _)| name.as_str()).collect();
        let mut lines: Vec<String> = existing
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !metric_names
                    .iter()
                    .any(|metric| trimmed.starts_with(metric))
            })
            .map(|line| line.to_string())
            .filter(|line| !line.trim().is_empty())
            .collect();
        for (name, value) in metrics {
            lines.push(format!("{name} {value}"));
        }
        lines.sort();
        let mut output = lines.join("\n");
        if !output.ends_with('\n') {
            output.push('\n');
        }
        fs::write(path, output)
    }
}

/// Deterministic property-testing primitives backed by a lightweight PRNG.
pub mod prop {
    use std::ops::RangeInclusive;
    use std::panic::{self, AssertUnwindSafe};

    /// Result type returned by property test registrations.
    pub type Result<T = ()> = std::result::Result<T, Failure>;

    /// Describes a failing property test invocation.
    #[derive(Debug, Clone)]
    pub struct Failure {
        name: String,
        iteration: Option<usize>,
        reason: String,
    }

    impl Failure {
        fn new(
            name: impl Into<String>,
            iteration: Option<usize>,
            reason: impl Into<String>,
        ) -> Self {
            Self {
                name: name.into(),
                iteration,
                reason: reason.into(),
            }
        }

        /// Renders the failure into a panic message.
        pub fn render(&self, test: &str) -> String {
            match self.iteration {
                Some(iter) => format!(
                    "property test `{test}` failed during `{}` iteration {iter}: {}",
                    self.name, self.reason
                ),
                None => format!(
                    "property test `{test}` failed during `{}`: {}",
                    self.name, self.reason
                ),
            }
        }
    }

    struct Case {
        body: Box<dyn FnMut() -> Result<()> + Send>,
    }

    struct RandomCase {
        iterations: usize,
        body: Box<dyn FnMut(&mut Rng) -> Result<()> + Send>,
    }

    /// Property-test runner that executes deterministic and pseudo-random
    /// cases.
    pub struct Runner {
        seed: u64,
        cases: Vec<Case>,
        random_cases: Vec<RandomCase>,
    }

    impl Default for Runner {
        fn default() -> Self {
            let seed = std::env::var("TB_PROP_SEED")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0x5EED1234_89ABCDEF);
            Self {
                seed,
                cases: Vec::new(),
                random_cases: Vec::new(),
            }
        }
    }

    impl Runner {
        /// Overrides the seed used for random cases.
        pub fn set_seed(&mut self, seed: u64) {
            self.seed = seed;
        }

        /// Registers a deterministic case that will be executed exactly once.
        pub fn add_case<F>(&mut self, name: impl Into<String>, mut body: F) -> Result<()>
        where
            F: FnMut() + Send + 'static,
        {
            let name_str = name.into();
            let case = Case {
                body: Box::new(move || Self::guard(name_str.clone(), || body())),
            };
            self.cases.push(case);
            Ok(())
        }

        /// Registers a random case executed `iterations` times using the
        /// internal PRNG.
        pub fn add_random_case<F>(
            &mut self,
            name: impl Into<String>,
            iterations: usize,
            mut body: F,
        ) -> Result<()>
        where
            F: FnMut(&mut Rng) + Send + 'static,
        {
            let name_str = name.into();
            let case = RandomCase {
                iterations: iterations.max(1),
                body: Box::new(move |rng| Self::guard(name_str.clone(), || body(rng))),
            };
            self.random_cases.push(case);
            Ok(())
        }

        fn guard<T, F>(name: String, body: F) -> Result<T>
        where
            F: FnOnce() -> T,
        {
            match panic::catch_unwind(AssertUnwindSafe(body)) {
                Ok(value) => Ok(value),
                Err(payload) => {
                    let reason = if let Some(msg) = payload.downcast_ref::<&str>() {
                        (*msg).to_string()
                    } else if let Some(msg) = payload.downcast_ref::<String>() {
                        msg.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    Err(Failure::new(name, None, reason))
                }
            }
        }

        /// Executes all registered cases. The first failure aborts the run and
        /// returns its diagnostic.
        pub fn run(&mut self) -> Result<()> {
            for case in &mut self.cases {
                (case.body)()?;
            }

            for (index, case) in self.random_cases.iter_mut().enumerate() {
                let mut rng = Rng::with_seed(self.seed ^ ((index as u64) << 32));
                for iter in 0..case.iterations {
                    match (case.body)(&mut rng) {
                        Ok(_) => {}
                        Err(mut failure) => {
                            failure.iteration = Some(iter);
                            return Err(failure);
                        }
                    }
                }
            }
            Ok(())
        }
    }

    /// Deterministic pseudo-random number generator used by the property
    /// harness.
    #[derive(Debug, Clone)]
    pub struct Rng {
        state: u64,
    }

    impl Rng {
        /// Constructs a generator seeded with the given value.
        pub fn with_seed(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            // LCG parameters from Numerical Recipes.
            self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
            self.state
        }

        /// Generates a boolean value.
        pub fn bool(&mut self) -> bool {
            (self.next_u64() & 1) == 1
        }

        /// Generates a `u8` within the given range (inclusive).
        pub fn range_u8(&mut self, range: RangeInclusive<u8>) -> u8 {
            self.sample_range(*range.start() as u64, *range.end() as u64) as u8
        }

        /// Generates a `u16` within the range.
        pub fn range_u16(&mut self, range: RangeInclusive<u16>) -> u16 {
            self.sample_range(*range.start() as u64, *range.end() as u64) as u16
        }

        /// Generates a `u32` within the range.
        pub fn range_u32(&mut self, range: RangeInclusive<u32>) -> u32 {
            self.sample_range(*range.start() as u64, *range.end() as u64) as u32
        }

        /// Generates a `u64` within the range.
        pub fn range_u64(&mut self, range: RangeInclusive<u64>) -> u64 {
            self.sample_range(*range.start(), *range.end())
        }

        /// Generates a `usize` within the range.
        pub fn range_usize(&mut self, range: RangeInclusive<usize>) -> usize {
            self.sample_range(*range.start() as u64, *range.end() as u64) as usize
        }

        fn sample_range(&mut self, start: u64, end: u64) -> u64 {
            if start == end {
                return start;
            }
            let width = end - start + 1;
            start + (self.next_u64() % width)
        }

        /// Produces a vector of random bytes with length within `len_range`.
        pub fn bytes(&mut self, len_range: RangeInclusive<usize>) -> Vec<u8> {
            let len = self.range_usize(len_range);
            (0..len).map(|_| self.range_u8(0..=u8::MAX)).collect()
        }
    }
}

/// Snapshot utilities storing textual baselines under `tests/snapshots` by
/// default. Set `TB_UPDATE_SNAPSHOTS=1` to rewrite stored values.
pub mod snapshot {
    use std::fs;
    use std::path::PathBuf;

    fn base_dir() -> PathBuf {
        std::env::var_os("TB_SNAPSHOT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tests/snapshots"))
    }

    fn file_path(module_path: &str, name: &str) -> PathBuf {
        let mut path = base_dir();
        for segment in module_path.split("::") {
            path.push(segment);
        }
        path.push(format!("{name}.snap"));
        path
    }

    fn normalize(input: &str) -> String {
        input.replace('\r', "")
    }

    /// Asserts that `value` matches the stored snapshot. Use
    /// `TB_UPDATE_SNAPSHOTS=1` to update the baseline.
    pub fn assert_snapshot(module_path: &str, name: &str, value: &(impl AsRef<str> + ?Sized)) {
        let value_str = normalize(value.as_ref());
        let path = file_path(module_path, name);
        if std::env::var("TB_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&path, value_str.as_bytes()).expect("write snapshot");
            return;
        }

        let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "snapshot `{}` missing. set TB_UPDATE_SNAPSHOTS=1 to record",
                path.display()
            )
        });
        if normalize(&expected) != value_str {
            panic!(
                "snapshot `{}` mismatch. run with TB_UPDATE_SNAPSHOTS=1 to update\nexpected:\n{}\nactual:\n{}",
                path.display(),
                expected,
                value_str
            );
        }
    }
}

/// Lightweight fixture helper returning the constructed value while allowing
/// downstream callers to opt into explicit teardown.
pub mod fixture {
    use std::ops::{Deref, DerefMut};

    /// Wrapper around a fixture value providing ergonomic access via `Deref`.
    #[derive(Debug)]
    pub struct Fixture<T> {
        value: T,
    }

    impl<T> Fixture<T> {
        /// Builds a new fixture wrapper.
        pub fn new(value: T) -> Self {
            Self { value }
        }
    }

    impl<T> Deref for Fixture<T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            &self.value
        }
    }

    impl<T> DerefMut for Fixture<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.value
        }
    }
}

/// Serial test helpers providing a global mutex.
pub mod serial {
    use std::sync::{Mutex, MutexGuard};

    static SERIAL_MUTEX: Mutex<()> = Mutex::new(());

    /// Locks the global mutex guarding serial tests.
    /// Recovers from poisoned mutex to prevent cascading test failures when one test panics.
    pub fn lock() -> MutexGuard<'static, ()> {
        SERIAL_MUTEX.lock().unwrap_or_else(|poisoned| {
            // Recover from poisoned mutex - when one test panics, subsequent tests should still run
            // This prevents cascading failures where all tests fail due to mutex poisoning
            poisoned.into_inner()
        })
    }
}

/// Declares a simple ignored unit test. Retained for compatibility with the
/// previous helper.
#[macro_export]
macro_rules! ignored_test {
    ($name:ident, $body:block) => {
        #[test]
        #[ignore]
        fn $name() {
            $body
        }
    };
}

/// Declares a benchmark target. The optional `iterations = <n>` argument allows
/// overriding the default iteration count.
#[macro_export]
macro_rules! tb_bench {
    ($name:ident, iterations = $iters:expr, $body:block) => {
        fn main() {
            $crate::bench::run(stringify!($name), $iters, || $body);
        }
    };
    ($name:ident, $body:block) => {
        fn main() {
            $crate::bench::run(stringify!($name), $crate::bench::DEFAULT_ITERATIONS, || {
                $body
            });
        }
    };
}

/// Declares a property test and exposes a mutable [`prop::Runner`] as the
/// block argument.
#[macro_export]
macro_rules! tb_prop_test {
    ($name:ident, |$runner:ident| $body:block) => {
        #[test]
        fn $name() {
            let mut $runner = $crate::prop::Runner::default();
            $body
            if let Err(failure) = $runner.run() {
                panic!("{}", failure.render(stringify!($name)));
            }
        }
    };
}

/// Declares a snapshot-oriented test.
#[macro_export]
macro_rules! tb_snapshot_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            $body
        }
    };
}

/// Asserts that a value matches the stored snapshot.
#[macro_export]
macro_rules! tb_snapshot {
    ($name:expr, $value:expr $(,)?) => {
        $crate::snapshot::assert_snapshot(module_path!(), $name, &$value);
    };
}

/// Declares a reusable fixture function that returns the constructed value
/// wrapped in [`fixture::Fixture`].
#[macro_export]
macro_rules! tb_fixture {
    ($name:ident, $body:block) => {
        #[allow(dead_code)]
        pub fn $name() -> $crate::fixture::Fixture<_> {
            $crate::fixture::Fixture::new($body)
        }
    };
}

pub use testkit_macros::tb_serial;

#[cfg(test)]
mod tests {
    use super::bench;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn bench_report_writes_prometheus_metric() {
        let mut path = PathBuf::from(env::temp_dir());
        path.push(format!(
            "tb_bench_metric_{}_{}.prom",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_str = path.to_string_lossy().to_string();
        env::set_var("TB_BENCH_PROM_PATH", &path_str);
        let _ = fs::remove_file(&path);
        bench::run("test_metric", 1, || {});
        env::remove_var("TB_BENCH_PROM_PATH");
        let contents = fs::read_to_string(&path).expect("benchmark metric written");
        assert!(contents.contains("benchmark_test_metric_seconds"));
        assert!(contents.contains("benchmark_test_metric_seconds_p50"));
        assert!(contents.contains("benchmark_test_metric_seconds_p90"));
        assert!(contents.contains("benchmark_test_metric_seconds_p99"));
        let _ = fs::remove_file(path);
    }
}
