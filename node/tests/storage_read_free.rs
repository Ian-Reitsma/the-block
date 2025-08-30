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
fn reads_do_not_burn() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("alice", 10);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = Arc::new(NoopProvider);
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider.clone());
    let data = vec![0u8; 512];
    let receipt = pipe.put_object(&data, "alice", &catalog).unwrap();
    let bal_after_write = Settlement::balance("alice");
    drop(pipe);
    let pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let _ = pipe.get_object(&receipt.manifest_hash).unwrap();
    assert_eq!(Settlement::balance("alice"), bal_after_write);
    Settlement::shutdown();
}
