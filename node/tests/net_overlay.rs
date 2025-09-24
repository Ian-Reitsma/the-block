#![cfg(feature = "integration-tests")]

use std::sync::Arc;

use p2p_overlay::OverlayService;
use tempfile::tempdir;
use the_block::config::{ensure_overlay_sanity, OverlayBackend, OverlayConfig};
use the_block::net::{self, discovery, uptime, OverlayAddress, OverlayPeerId};

struct OverlayRestore(Arc<dyn OverlayService<Peer = OverlayPeerId, Address = OverlayAddress>>);

impl Drop for OverlayRestore {
    fn drop(&mut self) {
        net::install_overlay(self.0.clone());
    }
}

#[test]
fn overlay_swap_end_to_end() {
    let previous = net::overlay_service();
    let _restore = OverlayRestore(previous);

    let dir = tempdir().unwrap();
    let path = dir.path().join("libp2p_peers.bin");
    let libp2p_cfg = OverlayConfig {
        backend: OverlayBackend::Libp2p,
        peer_db_path: path.to_string_lossy().into_owned(),
    };
    net::configure_overlay(&libp2p_cfg);
    ensure_overlay_sanity(&libp2p_cfg).expect("libp2p overlay sanity");

    let local = discovery::PeerId::random();
    let mut libp2p_discovery = discovery::new(local.clone());
    let other = discovery::PeerId::random();
    let addr: OverlayAddress = "/ip4/127.0.0.1/tcp/9100".parse().unwrap();
    libp2p_discovery.add_peer(other.clone(), addr.clone());
    libp2p_discovery.persist();
    uptime::note_seen(other.clone());

    let status = net::overlay_status();
    assert_eq!(status.backend, "libp2p");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 1);
    assert_eq!(
        status.database_path.as_deref(),
        Some(libp2p_cfg.peer_db_path.as_str()),
    );

    let mut stub_cfg = libp2p_cfg.clone();
    stub_cfg.backend = OverlayBackend::Stub;
    net::configure_overlay(&stub_cfg);
    ensure_overlay_sanity(&stub_cfg).expect("stub overlay sanity");

    let mut stub_discovery = discovery::new(discovery::PeerId::random());
    let stub_peer = discovery::PeerId::random();
    stub_discovery.add_peer(stub_peer.clone(), addr);
    stub_discovery.persist();
    uptime::note_seen(stub_peer);

    let status = net::overlay_status();
    assert_eq!(status.backend, "stub");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 1);
    assert!(status.database_path.is_none());
}
