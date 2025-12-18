use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use testkit::bench;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn bench_env_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

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
    let _guard = bench_env_guard();
    let history_path = unique_path("history");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    bench::run("history_record", 2, || {});
    bench::run("history_record", 2, || {});
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    let contents = fs::read_to_string(&history_path).expect("history written");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 3, "expected header plus at least two rows");
    let header_fields: Vec<&str> = lines[0].split(',').collect();
    assert!(header_fields.contains(&"per_iter_ewma_seconds"));
    let fields: Vec<&str> = lines[1].split(',').collect();
    assert!(
        fields.len() >= header_fields.len(),
        "expected percentile columns"
    );
    let regressions_idx = header_fields
        .iter()
        .position(|col| *col == "regressions")
        .expect("regressions column present");
    assert!(!fields[regressions_idx].is_empty());
    let per_iter_ewma_idx = header_fields
        .iter()
        .position(|col| *col == "per_iter_ewma_seconds")
        .expect("per_iter_ewma column present");
    assert!(
        fields[per_iter_ewma_idx]
            .parse::<f64>()
            .expect("per_iter ewma numeric")
            >= 0.0
    );
    let _ = fs::remove_file(&history_path);
}

#[test]
fn history_records_missing_percentiles_as_empty_fields() {
    let _guard = bench_env_guard();
    let history_path = unique_path("history_missing");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    bench::record_result(
        "history_missing_percentiles",
        bench::BenchResult {
            iterations: 1,
            elapsed: Duration::from_millis(5),
            samples: Vec::new(),
        },
    );
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    let contents = fs::read_to_string(&history_path).expect("history written");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 2, "expected header and data row");
    let header_fields: Vec<&str> = lines[0].split(',').collect();
    let row_fields: Vec<&str> = lines[1].split(',').collect();
    let blank_columns = [
        "p50_seconds",
        "p90_seconds",
        "p99_seconds",
        "p50_ewma_seconds",
        "p90_ewma_seconds",
        "p99_ewma_seconds",
    ];
    for column in blank_columns {
        let idx = header_fields
            .iter()
            .position(|col| *col == column)
            .unwrap_or_else(|| panic!("{column} column missing"));
        assert!(row_fields.get(idx).is_some(), "{column} value missing");
        assert!(
            row_fields[idx].is_empty(),
            "{column} should be empty when percentile data is absent"
        );
    }
    let _ = fs::remove_file(&history_path);
}

#[test]
fn history_ewma_persists_when_percentiles_missing() {
    let _guard = bench_env_guard();
    let history_path = unique_path("history_mixed");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    bench::record_result(
        "history_mixed_percentiles",
        bench::BenchResult {
            iterations: 2,
            elapsed: Duration::from_millis(6),
            samples: vec![Duration::from_millis(2), Duration::from_millis(4)],
        },
    );
    bench::record_result(
        "history_mixed_percentiles",
        bench::BenchResult {
            iterations: 1,
            elapsed: Duration::from_millis(5),
            samples: Vec::new(),
        },
    );
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    let contents = fs::read_to_string(&history_path).expect("history written");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 3, "expected header plus two rows");
    let header_fields: Vec<&str> = lines[0].split(',').collect();
    let last_fields: Vec<&str> = lines.last().unwrap().split(',').collect();
    let percentile_columns = ["p50_seconds", "p90_seconds", "p99_seconds"];
    for column in percentile_columns {
        let idx = header_fields
            .iter()
            .position(|col| *col == column)
            .unwrap_or_else(|| panic!("{column} column missing"));
        assert!(last_fields.get(idx).is_some(), "{column} value missing");
        assert!(
            last_fields[idx].is_empty(),
            "{column} should be empty on missing samples"
        );
    }
    let ewma_columns = [
        "per_iter_ewma_seconds",
        "p50_ewma_seconds",
        "p90_ewma_seconds",
        "p99_ewma_seconds",
    ];
    for column in ewma_columns {
        let idx = header_fields
            .iter()
            .position(|col| *col == column)
            .unwrap_or_else(|| panic!("{column} column missing"));
        assert!(
            last_fields
                .get(idx)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            "{column} should preserve previous EWMA value"
        );
    }
    let _ = fs::remove_file(&history_path);
}

#[test]
fn regression_thresholds_trigger_alert_files() {
    let _guard = bench_env_guard();
    let history_path = unique_path("history_regression");
    let alert_path = unique_path("alert");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_REGRESSION_THRESHOLDS", "per_iter=0");
    bench::run("threshold_regression", 1, || {
        std::thread::sleep(std::time::Duration::from_millis(1));
    });
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_REGRESSION_THRESHOLDS");
    let alert = fs::read_to_string(&alert_path).expect("alert written");
    assert!(alert.contains("per_iter"));
    let contents = fs::read_to_string(&history_path).expect("history retained");
    let lines: Vec<&str> = contents.lines().collect();
    assert!(lines.len() >= 2);
    let header_fields: Vec<&str> = lines[0].split(',').collect();
    let regressions_idx = header_fields
        .iter()
        .position(|col| *col == "regressions")
        .expect("regressions header present");
    let last_fields: Vec<&str> = lines.last().unwrap().split(',').collect();
    assert_eq!(last_fields.get(regressions_idx).copied(), Some("per_iter"));
    let _ = fs::remove_file(&history_path);
    let _ = fs::remove_file(&alert_path);
}

#[test]
fn malformed_threshold_entries_are_ignored() {
    let _guard = bench_env_guard();
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
    let _guard = bench_env_guard();
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

#[test]
fn threshold_directory_overrides_apply() {
    let _guard = bench_env_guard();
    let history_path = unique_path("history_config");
    let alert_path = unique_path("alert_config");
    let threshold_dir = unique_path("threshold_dir");
    fs::create_dir_all(&threshold_dir).expect("threshold dir");
    let config_path = threshold_dir.join("threshold_config.thresholds");
    fs::write(&config_path, "per_iter=0\n").expect("config written");
    std::env::set_var("TB_BENCH_HISTORY_PATH", &history_path);
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_THRESHOLD_DIR", &threshold_dir);
    bench::run("threshold_config", 1, || {
        thread::sleep(Duration::from_millis(1));
    });
    std::env::remove_var("TB_BENCH_HISTORY_PATH");
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_THRESHOLD_DIR");
    let alert = fs::read_to_string(&alert_path).expect("alert written");
    assert!(alert.contains("per_iter"));
    let contents = fs::read_to_string(&history_path).expect("history retained");
    let lines: Vec<&str> = contents.lines().collect();
    let header_fields: Vec<&str> = lines[0].split(',').collect();
    let regressions_idx = header_fields
        .iter()
        .position(|col| *col == "regressions")
        .expect("regressions header present");
    let last_fields: Vec<&str> = lines.last().unwrap().split(',').collect();
    assert_eq!(last_fields.get(regressions_idx).copied(), Some("per_iter"));
    let _ = fs::remove_file(&history_path);
    let _ = fs::remove_file(&alert_path);
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_dir(&threshold_dir);
}

#[test]
fn unknown_threshold_keys_are_ignored_with_warning() {
    let _guard = bench_env_guard();
    let alert_path = unique_path("alert_unknown");
    std::env::set_var("TB_BENCH_ALERT_PATH", &alert_path);
    std::env::set_var("TB_BENCH_REGRESSION_THRESHOLDS", "p42=1");
    bench::run("threshold_unknown", 1, || {});
    std::env::remove_var("TB_BENCH_ALERT_PATH");
    std::env::remove_var("TB_BENCH_REGRESSION_THRESHOLDS");
    assert!(
        !alert_path.exists(),
        "unknown thresholds should not trigger regression alerts"
    );
    let _ = fs::remove_file(&alert_path);
}
