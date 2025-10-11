use std::sync::{Mutex, MutexGuard, OnceLock};

use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

static SETTLEMENT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct SettlementGuard {
    _lock: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl SettlementGuard {
    fn new() -> Self {
        let lock = SETTLEMENT_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let dir = tempdir().expect("settlement tempdir");
        let path = dir.path().join("settlement");
        let path_str = path.to_str().expect("settlement path str");
        Settlement::init(path_str, SettleMode::DryRun);
        Self {
            _lock: lock,
            _dir: dir,
        }
    }

    fn prefund(&self, provider: &str, amount: u64) {
        Settlement::accrue(provider, "test_prefund", amount);
    }
}

impl Drop for SettlementGuard {
    fn drop(&mut self) {
        Settlement::shutdown();
    }
}

struct TestProvider {
    id: String,
    rtt: f64,
}

impl Provider for TestProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn send_chunk(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn probe(&self) -> Result<f64, String> {
        Ok(self.rtt)
    }
}

#[test]
fn selects_low_rtt_provider_for_first_chunk() {
    let _settlement = SettlementGuard::new();
    _settlement.prefund("lane", 1_000_000);
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("pipeline-db");
    let mut pipeline = StoragePipeline::open(path.to_str().expect("path"));

    let mut catalog = NodeCatalog::new();
    catalog.register(TestProvider {
        id: "fast".to_string(),
        rtt: 25.0,
    });
    catalog.register(TestProvider {
        id: "slow".to_string(),
        rtt: 220.0,
    });
    catalog.probe_and_prune();

    // Size chosen to require at least two chunks, exercising the placement policy.
    let data = vec![7u8; 1_600_000];
    let (receipt, _) = pipeline
        .put_object(&data, "lane", &mut catalog)
        .expect("store object");

    let manifest = pipeline
        .get_manifest(&receipt.manifest_hash)
        .expect("manifest");
    assert!(
        manifest.chunk_lens.len() >= 2,
        "expected multi-chunk manifest"
    );

    let mut fast_has_first = false;
    let mut slow_has_first = false;
    for entry in &manifest.provider_chunks {
        if entry.provider == "fast" {
            fast_has_first = entry.chunk_indices.contains(&0);
        }
        if entry.provider == "slow" {
            slow_has_first = entry.chunk_indices.contains(&0);
        }
    }
    assert!(fast_has_first, "fast provider should store first chunk");
    assert!(!slow_has_first, "slow provider should not host first chunk");
}

#[test]
fn maintenance_providers_are_skipped() {
    let _settlement = SettlementGuard::new();
    _settlement.prefund("lane", 1_000_000);
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("pipeline-maint");
    let mut pipeline = StoragePipeline::open(path.to_str().expect("path"));

    let mut catalog = NodeCatalog::new();
    catalog.register(TestProvider {
        id: "primary".to_string(),
        rtt: 40.0,
    });
    catalog.register(TestProvider {
        id: "maintenance".to_string(),
        rtt: 35.0,
    });
    catalog.probe_and_prune();

    pipeline
        .set_provider_maintenance("maintenance", true)
        .expect("set maintenance");

    let data = vec![3u8; 900_000];
    let (receipt, _) = pipeline
        .put_object(&data, "lane", &mut catalog)
        .expect("store object");

    let manifest = pipeline
        .get_manifest(&receipt.manifest_hash)
        .expect("manifest");
    assert!(manifest.provider_chunks.len() >= 1);
    assert!(manifest
        .provider_chunks
        .iter()
        .any(|entry| entry.provider == "primary"));
    assert!(manifest
        .provider_chunks
        .iter()
        .all(|entry| entry.provider != "maintenance"));
}
