#![cfg(feature = "integration-tests")]
use serial_test::serial;
use tempfile::tempdir;
use the_block::{
    config::{rate_limit_cfg, reputation_cfg, set_current, watch, NodeConfig},
    net::{p2p_max_per_sec, peer_reputation_decay, set_p2p_max_per_sec, set_peer_reputation_decay},
};

#[test]
#[serial]
fn reload_updates_limits() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let mut cfg = NodeConfig::default();
    cfg.p2p_max_per_sec = 50;
    cfg.peer_reputation_decay = 0.01;
    cfg.save(path).unwrap();
    set_current(cfg.clone());
    watch(path);
    set_p2p_max_per_sec(cfg.p2p_max_per_sec);
    set_peer_reputation_decay(cfg.peer_reputation_decay);

    // modify
    cfg.p2p_max_per_sec = 5;
    cfg.peer_reputation_decay = 0.5;
    cfg.save(path).unwrap();
    the_block::config::reload();
    assert_eq!(p2p_max_per_sec(), 5);
    assert!((peer_reputation_decay() - 0.5).abs() < 1e-6);
    assert_eq!(rate_limit_cfg().read().unwrap().p2p_max_per_sec, 5);
    assert!((reputation_cfg().read().unwrap().peer_reputation_decay - 0.5).abs() < 1e-6);
    // cleanup defaults
    set_p2p_max_per_sec(100);
    set_peer_reputation_decay(0.01);
    drop(dir);
}
