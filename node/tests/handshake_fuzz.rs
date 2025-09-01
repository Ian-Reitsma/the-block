use ed25519_dalek::SigningKey;
use proptest::prelude::*;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use the_block::{
    net::{Handshake, Message, Payload, PeerSet},
    Blockchain,
};

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[0u8; 32])
}

proptest! {
    #[test]
    fn fuzz_identifier_exchange(node_id in any::<[u8;32]>(), protocol_version in any::<u32>(), features in any::<u32>()) {
        let dir = tempdir().unwrap();
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let peers = PeerSet::new(vec![]);
        let hs = Handshake { node_id, protocol_version, features };
        let msg = Message::new(Payload::Handshake(hs), &sample_sk());
        // Should never panic regardless of contents
        peers.handle_message(msg, None, &bc);
    }
}
