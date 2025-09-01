mod util;

use serial_test::serial;
use std::process::Command;
use std::sync::Arc;
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;
#[cfg(feature = "telemetry")]
use the_block::telemetry::STORAGE_DISK_FULL_TOTAL;

#[derive(Clone, Copy)]
struct NoopProvider;

impl Provider for NoopProvider {
    fn id(&self) -> &str {
        "local"
    }
}

#[test]
#[serial]
fn disk_full_recovery() {
    util::rpc::randomize_client_timeout();
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    if Command::new("mount")
        .args(["-t", "tmpfs", "-o", "size=1M", "tmpfs", path])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("skipping: cannot mount tmpfs");
        return;
    }
    Settlement::init(path, SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(path);
    let provider = Arc::new(NoopProvider);
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider);
    // occupy most of tmpfs
    std::fs::write(dir.path().join("filler"), vec![0u8; 900_000]).unwrap();
    let data = vec![0u8; 200_000];
    let err = pipe.put_object(&data, "lane", &catalog).unwrap_err();
    assert!(err.contains("No space") || err.contains("disk"));
    #[cfg(feature = "telemetry")]
    assert_eq!(STORAGE_DISK_FULL_TOTAL.get(), 1);
    // free space and retry
    std::fs::remove_file(dir.path().join("filler")).unwrap();
    let receipt = pipe.put_object(&data, "lane", &catalog).unwrap();
    assert!(receipt.chunk_count > 0);
    Settlement::shutdown();
    let _ = Command::new("umount").arg(path).status();
}
