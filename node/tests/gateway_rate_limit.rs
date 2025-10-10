#![cfg(feature = "integration-tests")]
use std::time::{SystemTime, UNIX_EPOCH};

use sys::tempfile::tempdir;
use the_block::gateway::{
    http::{check, RateConfig},
    read_receipt::{append, reads_since},
};

#[test]
fn denies_after_burst_exhausted() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_GATEWAY_RECEIPTS", dir.path());
    let cfg = RateConfig {
        tokens_per_minute: 1.0,
        burst: 1.0,
    };
    assert!(check("1.2.3.4", None, "ex.com", "gw", &cfg));
    append("ex.com", "gw", 10, false, true).unwrap();
    assert!(!check("1.2.3.4", None, "ex.com", "gw", &cfg));
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 3600;
    let receipt_dir = dir.path().join("read").join(epoch.to_string());
    let count = std::fs::read_dir(receipt_dir).unwrap().count();
    assert_eq!(count, 2);
    let (total, _) = reads_since(0, "ex.com");
    assert_eq!(total, 1);
}
