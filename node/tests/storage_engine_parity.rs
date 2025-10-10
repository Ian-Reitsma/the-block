#![cfg(feature = "integration-tests")]

use concurrency::Lazy;
use ledger::address::ShardId;
use state::{MerkleTrie, SnapshotManager};
use std::collections::HashMap;
use std::sync::Mutex;
use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::gossip::{config::GossipConfig, relay::Relay};
use the_block::simple_db::{self, EngineConfig, EngineKind};

static ENGINE_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn supported_engines() -> Vec<EngineKind> {
    [EngineKind::Memory, EngineKind::RocksDb, EngineKind::Sled]
        .into_iter()
        .filter(|kind| kind.is_available())
        .collect()
}

fn configure_for_engine(engine: EngineKind) {
    simple_db::set_legacy_mode(false);
    let config = EngineConfig {
        default_engine: engine,
        overrides: HashMap::new(),
    };
    simple_db::configure_engines(config);
}

#[test]
fn gossip_relay_engine_parity() {
    let _guard = ENGINE_TEST_LOCK.lock().unwrap();
    for engine in supported_engines() {
        configure_for_engine(engine);
        let dir = tempdir().expect("gossip tempdir");
        let store_dir = dir.path().join(format!("gossip-{}", engine.label()));
        let mut cfg = GossipConfig::default();
        cfg.shard_store_path = store_dir.to_string_lossy().into_owned();
        let relay = Relay::new(cfg.clone());
        let shard: ShardId = 1;
        let mut peer = [0u8; 32];
        peer[0] = engine.label().as_bytes()[0];
        relay.register_peer(shard, peer);
        drop(relay);
        let relay = Relay::new(cfg);
        let status = relay.status();
        let expected = crypto_suite::hex::encode(peer);
        let peers = status
            .shard_affinity
            .into_iter()
            .find(|entry| entry.shard == shard)
            .map(|entry| entry.peers)
            .unwrap_or_default();
        assert!(peers.iter().any(|p| p == &expected));
    }
    simple_db::configure_engines(EngineConfig::default());
    simple_db::set_legacy_mode(false);
}

#[test]
fn settlement_engine_parity() {
    let _guard = ENGINE_TEST_LOCK.lock().unwrap();
    for engine in supported_engines() {
        configure_for_engine(engine);
        let dir = tempdir().expect("settlement tempdir");
        let base = dir.path().join(format!("settlement-{}", engine.label()));
        let base_str = base.to_str().expect("settlement path str");
        Settlement::init(base_str, SettleMode::Real);
        Settlement::accrue("provider-a", "test_accrue", 42);
        Settlement::shutdown();
        Settlement::init(base_str, SettleMode::Real);
        let balances = Settlement::balances();
        assert!(balances
            .iter()
            .any(|b| b.provider == "provider-a" && b.ct == 42));
        Settlement::shutdown();
    }
    simple_db::configure_engines(EngineConfig::default());
    simple_db::set_legacy_mode(false);
}

#[test]
fn snapshot_engine_parity() {
    let _guard = ENGINE_TEST_LOCK.lock().unwrap();
    for engine in supported_engines() {
        let dir = tempdir().expect("snapshot tempdir");
        let mut trie = MerkleTrie::new();
        trie.insert(b"alpha", b"beta");
        let manager = SnapshotManager::new_with_engine(
            dir.path().to_path_buf(),
            2,
            Some(engine.label().to_string()),
        );
        let path = manager.snapshot(&trie).expect("snapshot");
        let restored = manager.restore(&path).expect("restore");
        assert_eq!(restored.get(b"alpha").unwrap(), b"beta");
    }
}
