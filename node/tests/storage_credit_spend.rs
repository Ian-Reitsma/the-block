use std::sync::Arc;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

#[derive(Clone, Copy)]
struct NoopProvider;

impl Provider for NoopProvider {
    fn id(&self) -> &str {
        "local"
    }
}

#[test]
fn writes_burn_and_limit() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);
    Settlement::set_balance("alice", 1);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = Arc::new(NoopProvider);
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider.clone());
    let data = vec![0u8; 512];
    let _ = pipe.put_object(&data, "alice", &catalog).unwrap();
    assert_eq!(Settlement::balance("alice"), 0);
    let err = pipe.put_object(&data, "alice", &catalog).unwrap_err();
    assert_eq!(err, "ERR_STORAGE_QUOTA_CREDITS");
    Settlement::shutdown();
}
