mod util;

use serial_test::serial;
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
#[serial]
fn fails_when_disk_full() {
    util::rpc::randomize_client_timeout();
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    pipe.db_mut().set_byte_limit(1024);
    let provider = Arc::new(NoopProvider);
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider);
    let data = vec![0u8; 2048];
    let err = pipe.put_object(&data, "lane", &catalog).unwrap_err();
    assert!(err.contains("No space") || err.contains("disk"));
    Settlement::shutdown();
}
