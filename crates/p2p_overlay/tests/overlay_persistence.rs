use p2p_overlay::libp2p_overlay::{FileOverlayStore, Libp2pOverlay, Libp2pPeerId as PeerId};
use p2p_overlay::stub::{StubOverlay, StubPeerId};
use p2p_overlay::{OverlayService, OverlayStore};
use tempfile::tempdir;

#[test]
fn libp2p_overlay_persists_and_reloads_peers() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("peers.db");
    let overlay = Libp2pOverlay::new(path.clone());

    let local = PeerId::random();
    let mut discovery = overlay.discovery(local.clone());
    let remote = PeerId::random();
    let first_addr = libp2p::multiaddr::multiaddr!(Ip4([127, 0, 0, 1]), Tcp(1234u16));
    discovery.add_peer(remote.clone(), first_addr.clone());
    discovery.persist();

    // Update the peer with a new address and ensure only the latest entry remains.
    let second_addr = libp2p::multiaddr::multiaddr!(Ip4([10, 0, 0, 2]), Tcp(9999u16));
    discovery.add_peer(remote.clone(), second_addr.clone());
    discovery.persist();

    let status = overlay.diagnostics().expect("overlay diagnostics");
    assert_eq!(status.label, "libp2p");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 0);
    assert_eq!(
        status
            .database_path
            .as_ref()
            .expect("libp2p database path")
            .as_path(),
        path.as_path()
    );

    let reloaded = overlay.discovery(local);
    assert!(reloaded.has_peer(&remote));

    let store = FileOverlayStore::new(path);
    let persisted = store.load().expect("load persisted peers");
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].0, remote);
    assert_eq!(persisted[0].1, second_addr);
}

#[test]
fn stub_overlay_tracks_peers_without_libp2p() {
    let overlay = StubOverlay::new();
    let store = overlay.store();

    let local = StubPeerId::new(b"local");
    let mut discovery = overlay.discovery(local.clone());
    let remote = StubPeerId::new(b"remote");
    discovery.add_peer(remote.clone(), b"addr-a".to_vec());
    discovery.persist();

    let mut reload = overlay.discovery(local);
    assert!(reload.has_peer(&remote));

    reload.add_peer(remote.clone(), b"addr-b".to_vec());
    reload.persist();

    let persisted = store.load().expect("load memory store");
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].0, remote);
    assert_eq!(persisted[0].1, b"addr-b".to_vec());

    let status = overlay.diagnostics().expect("stub diagnostics");
    assert_eq!(status.label, "stub");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 0);
    assert!(status.database_path.is_none());
}
