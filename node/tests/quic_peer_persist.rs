#![cfg(feature = "integration-tests")]
use concurrency::Bytes;
use std::net::SocketAddr;
use the_block::net::PeerSet;
use the_block::p2p::handshake::Transport;

#[test]
fn quic_peer_persists() {
    let dir = sys::tempfile::tempdir().unwrap();
    let peer_db = dir.path().join("peers.txt");
    let quic_db = dir.path().join("quic_peers.txt");
    std::env::set_var("TB_PEER_DB_PATH", &peer_db);
    std::env::set_var("TB_QUIC_PEER_DB_PATH", &quic_db);
    let tcp: SocketAddr = "127.0.0.1:10000".parse().unwrap();
    let quic: SocketAddr = "127.0.0.1:10001".parse().unwrap();
    let cert = Bytes::from(vec![1, 2, 3]);
    {
        let peers = PeerSet::new(vec![tcp]);
        peers.set_quic(tcp, quic, cert.clone());
    }
    {
        let peers = PeerSet::new(vec![]);
        let list = peers.list_with_info();
        assert_eq!(list, vec![(quic, Transport::Quic, Some(cert))]);
    }
    std::env::remove_var("TB_PEER_DB_PATH");
    std::env::remove_var("TB_QUIC_PEER_DB_PATH");
}
