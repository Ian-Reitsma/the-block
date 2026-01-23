use p2p_overlay::inhouse_overlay::{
    InhouseDiscovery, InhouseOverlay, InhouseOverlayStore, InhousePeerId, PeerEndpoint,
};
use p2p_overlay::{Discovery, OverlayService, StubOverlay};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use sys::tempfile::tempdir;

fn peer(bytes: u8) -> InhousePeerId {
    let mut id = [0u8; 32];
    id.fill(bytes);
    InhousePeerId::new(id)
}

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[test]
fn inhouse_overlay_persists_and_loads_peers() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("overlay.json");
    let overlay = InhouseOverlay::new(path.clone());
    let mut discovery = overlay.discovery(peer(1));

    discovery.add_peer(peer(2), PeerEndpoint::new(addr(9000)));
    discovery.persist();

    let overlay = InhouseOverlay::new(path);
    let discovery = overlay.discovery(peer(1));
    assert!(discovery.has_peer(&peer(2)));
}

#[test]
fn discovery_nearest_peer_orders_by_distance() {
    let dir = tempdir().expect("tempdir");
    let store = InhouseOverlayStore::new(dir.path().join("dht.json"));
    let mut discovery = InhouseDiscovery::new(
        peer(9),
        store,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    discovery.add_peer(peer(1), PeerEndpoint::new(addr(9100)));
    discovery.add_peer(peer(255), PeerEndpoint::new(addr(9200)));
    discovery.add_peer(peer(128), PeerEndpoint::new(addr(9300)));

    let nearest = discovery.nearest_peers(&peer(200), 2);
    assert_eq!(nearest.len(), 2);
    assert_eq!(nearest[0].0, peer(255));
    assert_eq!(nearest[1].0, peer(128));
}

#[test]
fn stub_overlay_reports_metrics() {
    let overlay = StubOverlay::new();
    let diagnostics = overlay.diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.label, "stub");
    assert_eq!(diagnostics.persisted_peers, 0);
}
