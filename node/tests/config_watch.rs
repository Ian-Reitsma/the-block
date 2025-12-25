#![cfg(feature = "integration-tests")]

use std::fs::{self, File};
use std::time::Duration;

use foundation_serialization::toml;
use sys::tempfile::tempdir;
use the_block::config::{self, NodeConfig};

#[test]
fn config_watch_detects_changes() {
    let dir = tempdir().expect("tempdir");
    let default_path = dir.path().join("default.toml");
    let gossip_path = dir.path().join("gossip.toml");
    let storage_path = dir.path().join("storage.toml");

    let initial = NodeConfig {
        snapshot_interval: 5,
        ..Default::default()
    };
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
    fs::write(&default_path, &contents).expect("update default config");
    // Force filesystem to flush so kqueue sees the change immediately
    File::open(&default_path)
        .and_then(|f| f.sync_all())
        .expect("sync file");

    // Touch the directory to force kqueue notification on macOS (APFS doesn't immediately
    // update directory mtime when files change, so we manually trigger it)
    use std::os::unix::fs::PermissionsExt;
    let dir_meta = fs::metadata(dir.path()).expect("get dir metadata");
    let mut perms = dir_meta.permissions();
    let mode = perms.mode();
    perms.set_mode(mode); // Set to same value to trigger mtime update
    fs::set_permissions(dir.path(), perms).expect("touch directory");

    runtime::block_on(async {
        // Wait up to 5 seconds for config reload (kqueue + async scheduling can be slow)
        for _ in 0..100 {
            if config::current().snapshot_interval == 9 {
                return;
            }
            runtime::yield_now().await;
            runtime::sleep(Duration::from_millis(50)).await;
        }
        panic!("config watcher failed to reload default config");
    });
}
