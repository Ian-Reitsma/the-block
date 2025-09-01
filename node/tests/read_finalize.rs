use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::tempdir;
use the_block::{
    compute_market::settlement::{self, SettleMode, Settlement},
    credits::issuance,
    gateway::read_receipt::{append, batch},
};

#[test]
fn read_batches_finalize_and_issue() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_GATEWAY_RECEIPTS", dir.path());
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::Real, 0, 0.0, 0);
    issuance::seed_read_pool(100);
    append("ex.com", "prov1", 10, false, true).unwrap();
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 3600;
    let anchor = batch(epoch).unwrap();
    settlement::confirm_anchor(&anchor);
    let final_dir = dir.path().join("read").join(format!("{}.final", epoch));
    assert!(final_dir.exists());
    assert_eq!(Settlement::balance("prov1"), 10);
}
