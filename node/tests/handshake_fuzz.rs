use ed25519_dalek::SigningKey;
use proptest::prelude::*;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use the_block::{
    net::{self, Message, Payload, PeerSet},
    p2p::handshake::{Hello, Transport},
    Blockchain,
};

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[0u8; 32])
}

proptest! {
    #[test]
    fn fuzz_identifier_exchange(proto_version in any::<u16>(), feature_bits in any::<u32>()) {
        let dir = tempdir().unwrap();
        net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let peers = PeerSet::new(vec![]);
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version,
            feature_bits,
            agent: String::new(),
            nonce: 0,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sample_sk());
        // Should never panic regardless of contents
        peers.handle_message(msg, None, &bc);
    }
}

proptest! {
    #[test]
    fn fuzz_malformed_handshake(raw in proptest::collection::vec(any::<u8>(), 0..256)) {
        let dir = tempdir().unwrap();
        net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let peers = PeerSet::new(vec![]);
        if let Ok(msg) = bincode::deserialize::<Message>(&raw) {
            peers.handle_message(msg, None, &bc);
        }
    }
}
