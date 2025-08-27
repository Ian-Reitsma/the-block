use std::thread;
use std::time::Duration;

use tempfile::tempdir;
use the_block::storage::pipeline::{Provider, StoragePipeline};

struct MockProvider {
    id: String,
    bandwidth_bps: f64,
    rtt_ms: f64,
    loss_rate: std::sync::Mutex<f64>,
    loss_flip: Option<(usize, f64)>,
    sent: std::sync::Mutex<usize>,
}

impl MockProvider {
    fn new(
        id: &str,
        bandwidth_mbps: f64,
        rtt_ms: f64,
        loss: f64,
        loss_flip: Option<(usize, f64)>,
    ) -> Self {
        Self {
            id: id.to_string(),
            bandwidth_bps: bandwidth_mbps * 1_000_000.0 / 8.0,
            rtt_ms,
            loss_rate: std::sync::Mutex::new(loss),
            loss_flip,
            sent: std::sync::Mutex::new(0),
        }
    }
}

impl Provider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn send_chunk(&self, data: &[u8]) -> Result<(), String> {
        let secs = data.len() as f64 / self.bandwidth_bps;
        thread::sleep(Duration::from_secs_f64(secs));
        let mut s = self.sent.lock().unwrap();
        *s += 1;
        if let Some((thresh, new_loss)) = self.loss_flip {
            if *s >= thresh {
                *self.loss_rate.lock().unwrap() = new_loss;
            }
        }
        Ok(())
    }
    fn rtt_ewma(&self) -> f64 {
        self.rtt_ms
    }
    fn loss_ewma(&self) -> f64 {
        *self.loss_rate.lock().unwrap()
    }
}

#[test]
fn fast_provider_scales_up() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = MockProvider::new("fast", 50.0, 10.0, 0.0, None);
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &provider).unwrap();
    let profile = pipe.get_profile("fast").unwrap();
    assert!(profile.preferred_chunk >= 2 * 1024 * 1024);
}

#[test]
fn slow_provider_limits_size() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = MockProvider::new("slow", 5.0, 120.0, 0.0, None);
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &provider).unwrap();
    let profile = pipe.get_profile("slow").unwrap();
    assert!(profile.preferred_chunk <= 1024 * 1024);
    assert!(profile.preferred_chunk >= 512 * 1024);
}

#[test]
fn loss_triggers_downgrade() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = MockProvider::new("flaky", 50.0, 10.0, 0.0, Some((2, 0.05)));
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &provider).unwrap();
    let profile = pipe.get_profile("flaky").unwrap();
    assert!(profile.preferred_chunk <= 512 * 1024);
}

#[test]
fn profile_persists_across_restarts() {
    let dir = tempdir().unwrap();
    {
        let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let provider = MockProvider::new("persist", 50.0, 10.0, 0.0, None);
        let data = vec![0u8; 4 * 1024 * 1024];
        pipe.put_object(&data, "lane", &provider).unwrap();
        let profile = pipe.get_profile("persist").unwrap();
        assert!(profile.preferred_chunk >= 2 * 1024 * 1024);
    }
    {
        let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let profile = pipe.get_profile("persist").unwrap();
        let provider = MockProvider::new("persist", 50.0, 10.0, 0.0, None);
        let data = vec![0u8; 1024 * 1024];
        let receipt = pipe.put_object(&data, "lane", &provider).unwrap();
        if profile.preferred_chunk as usize > data.len() {
            assert_eq!(receipt.chunk_count, 1);
        } else {
            assert!(receipt.chunk_count > 1);
        }
    }
}
