use serial_test::serial;
use std::fs;
use tempfile::tempdir;
use the_block::compute_market::price_board::{
    backlog_adjusted_bid, bands, init, persist, record_price, reset, reset_path_for_test,
};

use tracing_test::traced_test;

#[test]
#[serial]
fn computes_bands() {
    reset();
    for p in [1, 2, 3, 4, 5] {
        record_price(p);
    }
    let b = bands().unwrap();
    assert_eq!(b.0, 2);
    assert_eq!(b.1, 3);
    assert_eq!(b.2, 4);
}

#[test]
#[serial]
fn backlog_adjusts_bid() {
    reset();
    for p in [10, 10, 10, 10] {
        record_price(p);
    }
    let adj = backlog_adjusted_bid(4).unwrap();
    assert!(adj > 10);
}

#[test]
#[serial]
#[traced_test]
fn persists_across_restart() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.bin").to_str().unwrap().to_string();
    init(path.clone(), 10);
    record_price(5);
    persist();
    assert!(logs_contain("saved price board"));
    reset();
    init(path.clone(), 10);
    assert!(logs_contain("loaded price board"));
    let b = bands().unwrap();
    assert_eq!(b.1, 5);
}

#[test]
#[serial]
#[traced_test]
fn resets_on_corrupted_file() {
    reset_path_for_test();
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.bin");
    fs::write(&path, b"bad").unwrap();
    init(path.to_str().unwrap().to_string(), 10);
    assert!(bands().is_none());
    assert!(logs_contain("failed to parse price board"));
}
