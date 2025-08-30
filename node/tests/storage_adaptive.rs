use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

struct MockProvider {
    id: String,
    bw_bps: f64,
    rtt_ms: f64,
}

impl MockProvider {
    fn new(id: &str, bw_mbps: f64, rtt_ms: f64) -> Self {
        Self {
            id: id.to_string(),
            bw_bps: bw_mbps * 1_000_000.0 / 8.0,
            rtt_ms,
        }
    }
}

impl Provider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn send_chunk(&self, data: &[u8]) -> Result<(), String> {
        let secs = data.len() as f64 / self.bw_bps;
        thread::sleep(Duration::from_secs_f64(secs));
        Ok(())
    }
    fn probe(&self) -> Result<f64, String> {
        Ok(self.rtt_ms)
    }
}

#[test]
fn fast_provider_scales_up() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = Arc::new(MockProvider::new("fast", 50.0, 10.0));
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider);
    catalog.probe_and_prune();
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &catalog).unwrap();
    let profile = pipe.get_profile("fast").unwrap();
    assert!(profile.preferred_chunk >= 1024 * 1024);
    Settlement::shutdown();
}

#[test]
fn slow_provider_limits_size() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0, 0);
    Settlement::set_balance("lane", 10_000);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = Arc::new(MockProvider::new("slow", 5.0, 120.0));
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(provider);
    catalog.probe_and_prune();
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &catalog).unwrap();
    let profile = pipe.get_profile("slow").unwrap();
    assert!(profile.preferred_chunk <= 1024 * 1024);
    assert!(profile.preferred_chunk >= 512 * 1024);
    Settlement::shutdown();
}
