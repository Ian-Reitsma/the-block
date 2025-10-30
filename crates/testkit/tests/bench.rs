use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        samples: Vec::new(),
    };
    assert_eq!(result.per_iteration(), Duration::from_secs(0));
}

#[test]
fn percentile_uses_sorted_samples() {
    let result = bench::BenchResult {
        iterations: 3,
        elapsed: Duration::from_millis(6),
        samples: vec![
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(3),
        ],
    };
    assert_eq!(result.percentile(0.5).unwrap(), Duration::from_millis(2));
    assert_eq!(result.percentile(0.9).unwrap(), Duration::from_millis(3));
}

fn unique_path(suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    std::env::temp_dir().join(format!(
        "the_block_testkit_{suffix}_{pid}_{nanos}",
        suffix = suffix
    ))
}

#[test]
fn history_records_runs_with_percentiles() {
    let history_path = unique_path("history");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    bench::run("history_record", 2, || {});
    bench::run("history_record", 2, || {});
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    let contents = fs::read_to_string(&history_path).expect("history written");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 3, "expected header plus at least two rows");
    assert!(lines[0].starts_with("timestamp"));
    let fields: Vec<&str> = lines[1].split(',').collect();
    assert!(fields.len() >= 8, "expected percentile columns");
    let regressions = fields.last().unwrap();
    assert!(!regressions.is_empty());
    let _ = fs::remove_file(&history_path);
}

#[test]
fn regression_thresholds_trigger_alert_files() {
    let history_path = unique_path("history_regression");
    let alert_path = unique_path("alert");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_REGRESSION_THRESHOLDS", "per_iter=0");
    bench::run("threshold_regression", 1, || {});
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_REGRESSION_THRESHOLDS");
    let alert = fs::read_to_string(&alert_path).expect("alert written");
    assert!(alert.contains("per_iter"));
    let contents = fs::read_to_string(&history_path).expect("history retained");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 2);
    let last_fields: Vec<&str> = lines.last().unwrap().split(',').collect();
    assert_eq!(last_fields.last().copied(), Some("per_iter"));
    let _ = fs::remove_file(&history_path);
    let _ = fs::remove_file(&alert_path);
}

#[test]
fn malformed_threshold_entries_are_ignored() {
    let alert_path = unique_path("alert_malformed");
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_REGRESSION_THRESHOLDS", "p50=abc,per_iter=");
    bench::run("threshold_ignore", 1, || {
        thread::sleep(Duration::from_millis(1));
    });
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_REGRESSION_THRESHOLDS");
    assert!(
        !alert_path.exists(),
        "malformed thresholds should not trigger alerts"
    );
}

#[test]
fn thresholds_are_case_insensitive() {
    let alert_path = unique_path("alert_ci");
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_REGRESSION_THRESHOLDS", "P50=0.0000001");
    bench::run("threshold_case", 1, || {
        thread::sleep(Duration::from_millis(1));
    });
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_REGRESSION_THRESHOLDS");
    let alert = fs::read_to_string(&alert_path).expect("alert written");
    assert!(alert.contains("p50"));
    let _ = fs::remove_file(&alert_path);
}
