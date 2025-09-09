use serial_test::serial;
#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use std::fs;
#[cfg(feature = "test-telemetry")]
use std::time::{Duration, Instant};
#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use tempfile::tempdir;
#[cfg(feature = "test-telemetry")]
use the_block::compute_market::price_board::init_with_clock;
use the_block::compute_market::price_board::{backlog_adjusted_bid, bands, record_price, reset};
#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use the_block::compute_market::price_board::{init, persist, reset_path_for_test};
use the_block::transaction::FeeLane;
#[cfg(feature = "test-telemetry")]
use the_block::util::test_clock::PausedClock;
#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use the_block::util::versioned_blob::{encode_blob, MAGIC_PRICE_BOARD};

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
use tracing_test::traced_test;

#[test]
#[serial]
fn computes_bands() {
    reset();
    for p in [1, 2, 3, 4, 5] {
        record_price(FeeLane::Consumer, p, 1.0);
    }
    let b = bands(FeeLane::Consumer).unwrap();
    assert_eq!(b.0, 2);
    assert_eq!(b.1, 3);
    assert_eq!(b.2, 4);
}

#[test]
#[serial]
fn backlog_adjusts_bid() {
    reset();
    for p in [10, 10, 10, 10] {
        record_price(FeeLane::Consumer, p, 1.0);
    }
    let adj = backlog_adjusted_bid(FeeLane::Consumer, 4).unwrap();
    assert!(adj > 10);
}

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
#[test]
#[serial]
#[traced_test]
fn persists_across_restart() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("board.v1.bin")
        .to_str()
        .unwrap()
        .to_string();
    init(path.clone(), 10, 30);
    record_price(FeeLane::Consumer, 5, 1.0);
    persist();
    reset();
    init(path.clone(), 10, 30);
    let b = bands(FeeLane::Consumer).unwrap();
    assert_eq!(b.1, 5);
}

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
#[test]
#[serial]
#[traced_test]
fn resets_on_corrupted_file() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.v1.bin");
    fs::write(&path, b"bad").unwrap();
    init(path.to_str().unwrap().to_string(), 10, 30);
    assert!(bands(FeeLane::Consumer).is_none());
}

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
#[test]
#[serial]
#[traced_test]
fn ignores_tmp_crash_file() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.v1.bin");
    let path_str = path.to_str().unwrap().to_string();
    init(path_str.clone(), 10, 30);
    record_price(FeeLane::Consumer, 7, 1.0);
    persist();
    // Simulate crash leaving .tmp behind
    let tmp = path.with_extension("v1.bin.tmp");
    fs::write(&tmp, b"partial").unwrap();
    reset();
    init(path_str.clone(), 10, 30);
    let b = bands(FeeLane::Consumer).unwrap();
    assert_eq!(b.1, 7);
}

#[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
#[test]
#[serial]
#[traced_test]
fn resets_on_unknown_version() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.v1.bin");
    let blob = encode_blob(MAGIC_PRICE_BOARD, 999, &[]);
    fs::write(&path, blob).unwrap();
    init(path.to_str().unwrap().to_string(), 10, 30);
    assert!(bands(FeeLane::Consumer).is_none());
}

#[cfg(feature = "test-telemetry")]
#[test]
#[serial]
#[traced_test]
fn save_occurs_after_interval() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.v1.bin");
    let path_str = path.to_str().unwrap().to_string();
    let clock = PausedClock::new(Instant::now());
    init_with_clock(path_str.clone(), 10, 5, clock.clone());
    record_price(FeeLane::Consumer, 9, 1.0);
    clock.advance(Duration::from_secs(5));
    persist();
    assert!(fs::metadata(&path).is_ok());
}
