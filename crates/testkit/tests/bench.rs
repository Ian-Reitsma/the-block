use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use testkit::bench;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn run_executes_body_requested_iterations() {
    COUNTER.store(0, Ordering::SeqCst);
    bench::run("count", 5, || {
        COUNTER.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(COUNTER.load(Ordering::SeqCst), 5);
}

#[test]
fn per_iteration_gracefully_handles_zero_iterations() {
    let result = bench::BenchResult {
        iterations: 0,
        elapsed: Duration::from_secs(10),
    };
    assert_eq!(result.per_iteration(), Duration::from_secs(0));
}
