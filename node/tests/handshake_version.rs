#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use rand::{rngs::OsRng, RngCore};
use tempfile::tempdir;
use the_block::net::{self, Message, Payload, PeerSet, SUPPORTED_VERSION};
use the_block::p2p::handshake::{Hello, Transport};
use the_block::Blockchain;

#[test]
fn rejects_wrong_version() {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    let peers = PeerSet::new(vec![]);
    let mut bytes = [0u8; 32];
    OsRng::default().fill_bytes(&mut bytes);
    let kp = SigningKey::from_bytes(&bytes);
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: SUPPORTED_VERSION + 1,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),

        quic_provider: None,

        quic_capabilities: Vec::new(),
    };
    let msg = Message::new(Payload::Handshake(hello), &kp);
    let chain = std::sync::Arc::new(std::sync::Mutex::new(Blockchain::default()));
    peers.handle_message(msg, None, &chain);
    assert!(peers.list().is_empty());
}
