#![cfg(feature = "integration-tests")]
use libp2p::{multiaddr::multiaddr, PeerId};
use tempfile::tempdir;
use the_block::net::discovery::Discovery;

#[test]
fn persist_and_load_peers() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("peers.db");
    let local = PeerId::random();
    let mut d = Discovery::new(local, path.to_str().unwrap());
    let other = PeerId::random();
    let addr = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(1234u16));
    d.add_peer(other, addr);
    d.persist();
    let d2 = Discovery::new(local, path.to_str().unwrap());
    assert!(d2.has_peer(&other));
}
