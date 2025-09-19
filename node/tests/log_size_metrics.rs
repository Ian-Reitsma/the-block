#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use the_block::{gather_metrics, telemetry};

#[test]
fn log_size_histogram_exposed() {
    telemetry::reset_log_counters();
    telemetry::observe_log_size(128);
    let metrics = gather_metrics().unwrap();
    assert!(metrics.contains("log_size_bytes_sum"));
    assert!(metrics.contains("log_size_bytes_count"));
}
