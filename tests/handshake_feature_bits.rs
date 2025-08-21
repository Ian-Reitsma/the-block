#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use rand::{rngs::OsRng, RngCore};
use the_block::{
    net::{
        Handshake, Message, Payload, PeerSet, COMPUTE_MARKET_V1, PROTOCOL_VERSION,
        REQUIRED_FEATURES,
    },
    Blockchain,
};

#[test]
fn handshake_requires_compute_market_bit() {
    let peers = PeerSet::new(vec![]);
    let bc = Arc::new(Mutex::new(Blockchain::default()));
    let addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);

    // Missing compute-market bit should be rejected.
    let hs = Handshake {
        node_id: sk.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: REQUIRED_FEATURES & !COMPUTE_MARKET_V1,
    };
    let msg = Message::new(Payload::Handshake(hs), &sk);
    peers.handle_message(msg, Some(addr), &bc);
    assert!(!peers.list().contains(&addr));

    // Including the bit allows the peer to be added.
    let hs_ok = Handshake {
        node_id: sk.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: REQUIRED_FEATURES,
    };
    let msg_ok = Message::new(Payload::Handshake(hs_ok), &sk);
    peers.handle_message(msg_ok, Some(addr), &bc);
    assert!(peers.list().contains(&addr));
}
