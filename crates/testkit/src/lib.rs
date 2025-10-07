#![forbid(unsafe_code)]

//! First-party test harness primitives that replace the previous dependency on
//! external benchmarking, property-testing, and snapshot crates. The goal is
//! to provide pragmatic, deterministic tooling that keeps the workspace free
//! from third-party test harnesses while still exercising meaningful coverage.

/// Benchmark helpers that execute the provided closure a fixed number of
/// iterations and emit human-readable timing summaries.
pub mod bench {
    use std::time::{Duration, Instant};

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
    }

    impl BenchResult {
        /// Average duration per iteration.
        pub fn per_iteration(&self) -> Duration {
            if self.iterations == 0 {
                return Duration::from_secs(0);
            }
            self.elapsed / self.iterations as u32
        }
    }

    /// Runs a benchmark by executing `body` `iterations` times.
    pub fn run<F>(name: &str, iterations: usize, mut body: F)
    where
        F: FnMut(),
    {
        let iterations = iterations.max(1);
        let start = Instant::now();
        for _ in 0..iterations {
            body();
        }
        let elapsed = start.elapsed();
        report(
            name,
            BenchResult {
                iterations,
                elapsed,
            },
        );
    }

    fn report(name: &str, result: BenchResult) {
        let per_iter = result.per_iteration();
        println!(
            "benchmark `{name}`: {iters} iters in {total:?} ({avg:?}/iter)",
            iters = result.iterations,
            total = result.elapsed,
            avg = per_iter
        );
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
    pub fn lock() -> MutexGuard<'static, ()> {
        SERIAL_MUTEX.lock().expect("serial test mutex poisoned")
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
