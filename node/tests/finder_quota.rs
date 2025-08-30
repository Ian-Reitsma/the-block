use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::StoragePipeline;

#[test]
fn quota_matches_balance() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("test_provider", 10);
    assert_eq!(
        StoragePipeline::logical_quota_bytes("test_provider"),
        10 * 1024
    );
    Settlement::shutdown();
}
