use serial_test::serial;
use tempfile::tempdir;
use the_block::compute_market::price_board::{backlog_adjusted_bid, bands, record_price, reset, init, persist};

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
fn persists_across_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("board.bin").to_str().unwrap().to_string();
    init(path.clone(), 10);
    record_price(5);
    persist();
    reset();
    init(path, 10);
    let b = bands().unwrap();
    assert_eq!(b.1, 5);
}
