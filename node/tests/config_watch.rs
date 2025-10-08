#![cfg(feature = "integration-tests")]

use std::fs;
use std::time::Duration;

use foundation_serialization::toml;
use runtime;
use tempfile::tempdir;
use the_block::config::{self, NodeConfig};

#[test]
fn config_watch_detects_changes() {
    let dir = tempdir().expect("tempdir");
    let default_path = dir.path().join("default.toml");
    let gossip_path = dir.path().join("gossip.toml");
    let storage_path = dir.path().join("storage.toml");

    let mut initial = NodeConfig::default();
    initial.snapshot_interval = 5;
    let contents = toml::to_string_pretty(&initial).expect("encode config");
    fs::write(&default_path, contents).expect("write default config");
    fs::write(&gossip_path, "{}").expect("write gossip config");
    fs::write(&storage_path, "{}").expect("write storage config");

    config::set_current(initial.clone());
    config::watch(dir.path().to_str().expect("dir path"));

    runtime::block_on(async {
        runtime::sleep(Duration::from_millis(100)).await;
    });

    let mut updated = initial.clone();
    updated.snapshot_interval = 9;
    let contents = toml::to_string_pretty(&updated).expect("encode config");
    fs::write(&default_path, contents).expect("update default config");

    runtime::block_on(async {
        for _ in 0..40 {
            if config::current().snapshot_interval == 9 {
                return;
            }
            runtime::sleep(Duration::from_millis(50)).await;
        }
        panic!("config watcher failed to reload default config");
    });
}
