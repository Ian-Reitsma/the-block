mod util;

use hex::encode;
use rand::{rngs::OsRng, RngCore};
use serial_test::serial;
use std::sync::Arc;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

#[derive(Clone)]
struct LocalProvider {
    id: String,
}

impl Provider for LocalProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

#[test]
#[serial]
fn recovers_from_missing_shard() {
    util::rpc::randomize_client_timeout();
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let prov = Arc::new(LocalProvider { id: "p1".into() });
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(prov.clone());
    let mut data = vec![0u8; 1024];
    OsRng.fill_bytes(&mut data);
    let receipt = pipe.put_object(&data, "lane", &catalog).expect("store");
    // delete first shard
    let manifest = pipe.get_manifest(&receipt.manifest_hash).unwrap();
    let parity = format!("chunk/{}", encode(manifest.chunks[1].id));
    pipe.db_mut().remove(&parity);
    let out = pipe.get_object(&receipt.manifest_hash).expect("recover");
    assert_eq!(out, data);
    Settlement::shutdown();
}
