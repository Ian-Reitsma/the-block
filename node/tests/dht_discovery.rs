#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::config::OverlayConfig;
use the_block::net;
use the_block::net::discovery::{self, PeerId};
use the_block::net::OverlayAddress;

fn seeded_peer(seed: u8) -> PeerId {
    net::overlay_peer_from_bytes(&[seed; 32]).expect("overlay peer")
}

#[test]
fn persist_and_load_peers() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("peers.json");
    let previous = net::overlay_service();
    net::configure_overlay(&OverlayConfig {
        peer_db_path: path.to_string_lossy().into_owned(),
        ..OverlayConfig::default()
    });
    let local = seeded_peer(21);
    let mut d = discovery::new(local.clone());
    let other = seeded_peer(22);
    let addr = OverlayAddress::new("127.0.0.1:1234".parse().unwrap());
    d.add_peer(other.clone(), addr);
    d.persist();
    let d2 = discovery::new(local);
    assert!(d2.has_peer(&other));
    net::install_overlay(previous);
}
