use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

struct MockProvider {
    id: String,
    probe_result: Mutex<Result<f64, String>>,
    sent: Mutex<usize>,
}

impl MockProvider {
    fn new(id: &str, probe: Result<f64, String>) -> Self {
        Self {
            id: id.to_string(),
            probe_result: Mutex::new(probe),
            sent: Mutex::new(0),
        }
    }
}

impl Provider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn send_chunk(&self, _data: &[u8]) -> Result<(), String> {
        *self.sent.lock().unwrap() += 1;
        Ok(())
    }
    fn probe(&self) -> Result<f64, String> {
        self.probe_result.lock().unwrap().clone()
    }
}

#[test]
fn unhealthy_nodes_skipped() {
    let dir = tempdir().unwrap();
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun, 0, 0.0);
    Settlement::set_balance("lane", 10);
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let good = Arc::new(MockProvider::new("good", Ok(50.0)));
    let bad = Arc::new(MockProvider::new("bad", Err("timeout".into())));
    let mut catalog = NodeCatalog::new();
    catalog.register_arc(good.clone());
    catalog.register_arc(bad.clone());
    catalog.probe_and_prune();
    let data = vec![0u8; 1024];
    pipe.put_object(&data, "lane", &catalog).unwrap();
    assert_eq!(*good.sent.lock().unwrap(), 2);
    assert_eq!(*bad.sent.lock().unwrap(), 0);
    Settlement::shutdown();
}
