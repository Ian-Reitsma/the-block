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
    pipe.put_object(&data, "lane", &[&provider]).unwrap();
    let profile = pipe.get_profile("fast").unwrap();
    assert!(profile.preferred_chunk >= 2 * 1024 * 1024);
}

#[test]
fn slow_provider_limits_size() {
    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let provider = MockProvider::new("slow", 5.0, 120.0, 0.0, None);
    let data = vec![0u8; 4 * 1024 * 1024];
    pipe.put_object(&data, "lane", &[&provider]).unwrap();
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
    pipe.put_object(&data, "lane", &[&provider]).unwrap();
    let profile = pipe.get_profile("flaky").unwrap();
    assert!(profile.preferred_chunk <= 512 * 1024);
}

#[test]
fn profile_persists_across_restarts() {
    let dir = tempdir().unwrap();
    let preferred_chunk;
    {
        let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let provider = MockProvider::new("persist", 50.0, 10.0, 0.0, None);
        let data = vec![0u8; 4 * 1024 * 1024];
        pipe.put_object(&data, "lane", &[&provider]).unwrap();
        let profile = pipe.get_profile("persist").unwrap();
        assert!(profile.preferred_chunk >= 2 * 1024 * 1024);
        preferred_chunk = profile.preferred_chunk;
    }
    {
        let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let profile = pipe.get_profile("persist").unwrap();
        assert_eq!(profile.preferred_chunk, preferred_chunk);
        let provider = MockProvider::new("persist", 50.0, 10.0, 0.0, None);
        let data = vec![0u8; 1024 * 1024];
        let receipt = pipe.put_object(&data, "lane", &[&provider]).unwrap();
        let expected = ((data.len() as u32 + preferred_chunk - 1) / preferred_chunk) * 2;
        assert_eq!(receipt.chunk_count, expected);
    }
}

#[test]
fn profile_persists_across_multiple_restarts() {
    let dir = tempdir().unwrap();
    let expected_chunk;
    {
        let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let provider = MockProvider::new("twice", 50.0, 10.0, 0.0, None);
        let data = vec![0u8; 2 * 1024 * 1024];
        let receipt = pipe.put_object(&data, "lane", &[&provider]).unwrap();
        let profile = pipe.get_profile("twice").unwrap();
        expected_chunk = profile.preferred_chunk;
        let expected = ((data.len() as u32 + expected_chunk - 1) / expected_chunk) * 2;
        assert_eq!(receipt.chunk_count, expected);
    }
    {
        let pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let profile = pipe.get_profile("twice").unwrap();
        assert_eq!(profile.preferred_chunk, expected_chunk);
    }
    {
        let pipe = StoragePipeline::open(dir.path().to_str().unwrap());
        let profile = pipe.get_profile("twice").unwrap();
        assert_eq!(profile.preferred_chunk, expected_chunk);
    }
}

#[test]
fn multi_provider_manifest_records_mapping() {
    use rand::Rng;
    struct DirProvider {
        id: String,
        dir: tempfile::TempDir,
    }
    impl Provider for DirProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn send_chunk(&self, data: &[u8]) -> Result<(), String> {
            let path = self
                .dir
                .path()
                .join(rand::thread_rng().gen::<u64>().to_string());
            std::fs::write(path, data).map_err(|e| e.to_string())
        }
    }

    let dir = tempdir().unwrap();
    let mut pipe = StoragePipeline::open(dir.path().to_str().unwrap());
    let p1 = DirProvider {
        id: "prov1".into(),
        dir: tempdir().unwrap(),
    };
    let p2 = DirProvider {
        id: "prov2".into(),
        dir: tempdir().unwrap(),
    };
    let data = vec![0u8; 2 * 1024 * 1024];
    let receipt = pipe.put_object(&data, "lane", &[&p1, &p2]).unwrap();
    // ensure each provider received at least one shard
    assert!(std::fs::read_dir(p1.dir.path()).unwrap().next().is_some());
    assert!(std::fs::read_dir(p2.dir.path()).unwrap().next().is_some());
    let manifest = pipe.get_manifest(&receipt.manifest_hash).unwrap();
    let providers: std::collections::HashSet<_> = manifest
        .chunks
        .iter()
        .flat_map(|c| c.nodes.clone())
        .collect();
    assert!(providers.contains("prov1") && providers.contains("prov2"));
}
