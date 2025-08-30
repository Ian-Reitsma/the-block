use rand::{rngs::OsRng, RngCore};
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
fn put_and_get_roundtrip() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);
    Settlement::set_balance("consumer", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = Arc::new(NoopProvider);
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider.clone());
    let mut data = vec![0u8; 1024 * 1024];
    OsRng.fill_bytes(&mut data);
    let receipt = pipe.put_object(&data, "consumer", &catalog).expect("store");
    drop(pipe);
    let pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let out = pipe.get_object(&receipt.manifest_hash).expect("load");
    assert_eq!(out, data);
    Settlement::shutdown();
}
