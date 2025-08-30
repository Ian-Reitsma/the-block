use ed25519_dalek::SigningKey;
use rand::{rngs::OsRng, RngCore};
use the_block::net::{Handshake, Message, Payload, PeerSet, SUPPORTED_VERSION};
use the_block::Blockchain;

#[test]
fn rejects_wrong_version() {
    let peers = PeerSet::new(vec![]);
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let kp = SigningKey::from_bytes(&bytes);
    let hs = Handshake {
        node_id: [0u8; 32],
        protocol_version: SUPPORTED_VERSION + 1,
        features: 0,
    };
    let msg = Message::new(Payload::Handshake(hs), &kp);
    let chain = std::sync::Arc::new(std::sync::Mutex::new(Blockchain::default()));
    peers.handle_message(msg, None, &chain);
    assert!(peers.list().is_empty());
}
