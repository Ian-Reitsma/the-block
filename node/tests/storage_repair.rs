use hex::encode;
use std::time::Duration;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;
use the_block::storage::repair;

#[derive(Clone)]
struct NoopProvider;

impl Provider for NoopProvider {
    fn id(&self) -> &str {
        "local"
    }
}

#[tokio::test]
async fn rebuilds_missing_shard() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = NoopProvider;
    let mut catalog = NodeCatalog::new();
    catalog.register(provider);
    let data = vec![0u8; 1024];
    let receipt = pipe.put_object(&data, "lane", &catalog).unwrap();
    // remove a shard
    let manifest = pipe.get_manifest(&receipt.manifest_hash).unwrap();
    let missing = format!("chunk/{}", encode(manifest.chunks[0].id));
    pipe.db_mut().remove(&missing);
    repair::spawn(
        dir.path().to_str().unwrap().to_string(),
        Duration::from_millis(10),
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    let out = pipe.get_object(&receipt.manifest_hash).unwrap();
    assert_eq!(out, data);
    Settlement::shutdown();
}
