#![cfg(feature = "integration-tests")]
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

struct DummyProvider {
    id: String,
}
impl Provider for DummyProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

#[test]
fn rent_escrow_deposit_refund() {
    Settlement::init("", SettleMode::Real);
    Settlement::accrue("alice", "init", 100);

    let dir = tempdir().unwrap();
    let mut pipeline = StoragePipeline::open(dir.path().to_str().unwrap());
    pipeline.set_rent_rate(1); // 1 CT per byte

    let mut catalog = NodeCatalog::new();
    catalog.register(DummyProvider { id: "p1".into() });

    let (receipt, _blob_tx) = pipeline
        .put_object(b"hello", "alice", &catalog)
        .expect("store blob");

    assert_eq!(Settlement::balance("alice"), 95);

    let refund = pipeline
        .delete_object(&receipt.manifest_hash)
        .expect("delete blob");
    // 90% refund of 5 bytes => 4
    assert_eq!(refund, 4);
    assert_eq!(Settlement::balance("alice"), 99);
}
