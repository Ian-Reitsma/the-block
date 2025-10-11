use std::sync::{Mutex, OnceLock};

use coding::{CompressionConfig, Config, ErasureConfig};
use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::simple_db::{names, SimpleDb};
use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;
use the_block::storage::repair::{self, RepairLog, RepairRequest};
use the_block::storage::settings;
use the_block::storage::types::Redundancy;

struct ConfigGuard {
    previous: Config,
}

impl ConfigGuard {
    fn apply(config: Config) -> Self {
        let previous = settings::current();
        settings::configure(config);
        Self { previous }
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        settings::configure(self.previous.clone());
    }
}

struct SettlementGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl SettlementGuard {
    fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let dir = tempdir().expect("settlement tempdir");
        let path = dir.path().join("settlement");
        Settlement::init(path.to_str().expect("settlement path"), SettleMode::DryRun);
        Self {
            _lock: guard,
            _dir: dir,
        }
    }

    fn prefund(&self, provider: &str, amount: u64) {
        Settlement::accrue(provider, "fallback_prefund", amount);
    }
}

impl Drop for SettlementGuard {
    fn drop(&mut self) {
        Settlement::shutdown();
    }
}

#[derive(Clone)]
struct LoopbackProvider {
    id: String,
}

impl Provider for LoopbackProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn send_chunk(&self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn pipeline_repair_round_trip_with_xor_coder() {
    static CODING_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _coding_guard = CODING_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    let mut config = Config::default();
    config.erasure = ErasureConfig {
        algorithm: "xor".to_string(),
        data_shards: 4,
        parity_shards: 1,
    };
    config.compression = CompressionConfig {
        algorithm: "rle".to_string(),
        level: 0,
    };
    config.rollout.allow_fallback_coder = true;
    config.rollout.allow_fallback_compressor = true;
    let _config_guard = ConfigGuard::apply(config);

    let settlement = SettlementGuard::new();
    settlement.prefund("lane", 1_000_000);

    let dir = tempdir().expect("pipeline tempdir");
    let path = dir.path().join("pipeline");
    let path_str = path.to_str().expect("pipeline path str");
    let mut pipeline = StoragePipeline::open(path_str);

    let mut catalog = NodeCatalog::new();
    catalog.register(LoopbackProvider {
        id: "loopback".to_string(),
    });
    catalog.probe_and_prune();

    let data = vec![0x5Au8; 256 * 1024];
    let (receipt, _) = pipeline
        .put_object(&data, "lane", &mut catalog)
        .expect("store object");

    let manifest = pipeline
        .get_manifest(&receipt.manifest_hash)
        .expect("manifest");
    assert_eq!(
        manifest.redundancy,
        Redundancy::ReedSolomon { data: 4, parity: 1 }
    );
    assert_eq!(manifest.erasure_alg.as_deref(), Some("xor"));

    let mut db = SimpleDb::open_named(names::STORAGE_PIPELINE, path_str);
    let shard_id = manifest.chunks[0].id;
    db.remove(&format!("chunk/{}", crypto_suite::hex::encode(shard_id)));

    let log_dir = dir.path().join("repair_log");
    let log = RepairLog::new(&log_dir);
    let mut repair_db = SimpleDb::open_named(names::STORAGE_PIPELINE, path_str);
    let summary =
        repair::run_once(&mut repair_db, &log, RepairRequest::default()).expect("repair run");
    assert_eq!(summary.successes, 1);
    assert!(summary.failures == 0);

    let restored = db
        .get(&format!("chunk/{}", crypto_suite::hex::encode(shard_id)))
        .expect("restored shard");
    assert!(!restored.is_empty());
}
