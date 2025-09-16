#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use rand::{rngs::OsRng, RngCore};
use tempfile::tempdir;
use the_block::{
    net::{Message, Payload, PeerSet, COMPUTE_MARKET_V1, PROTOCOL_VERSION, REQUIRED_FEATURES},
    p2p::handshake::{Hello, Transport},
    Blockchain,
};

#[test]
fn handshake_requires_compute_market_bit() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    let peers = PeerSet::new(vec![]);
    let bc = Arc::new(Mutex::new(Blockchain::default()));
    let addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);

    // Missing compute-market bit should be rejected.
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: REQUIRED_FEATURES & !COMPUTE_MARKET_V1,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let msg = Message::new(Payload::Handshake(hello), &sk);
    peers.handle_message(msg, Some(addr), &bc);
    assert!(!peers.list().contains(&addr));

    // Including the bit allows the peer to be added.
    let hello_ok = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 1,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let msg_ok = Message::new(Payload::Handshake(hello_ok), &sk);
    peers.handle_message(msg_ok, Some(addr), &bc);
    assert!(peers.list().contains(&addr));
}
