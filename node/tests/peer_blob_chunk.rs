use ed25519_dalek::SigningKey;
use rand::{rngs::OsRng, RngCore};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use the_block::net::{BlobChunk, Message, Payload, PeerSet, REQUIRED_FEATURES, SUPPORTED_VERSION};
use the_block::p2p::handshake::{Hello, Transport};
use the_block::Blockchain;
use the_block::SimpleDb;

#[test]
fn shard_rate_limiting() {
    let dir = tempdir().unwrap();
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    std::env::set_var("TB_CHUNK_DB_PATH", dir.path().join("chunks"));
    std::env::set_var("TB_P2P_SHARD_RATE", "512");
    std::env::set_var("TB_P2P_SHARD_BURST", "512");

    let peers = PeerSet::new(vec![]);
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let sk = SigningKey::from_bytes(&bytes);
    let addr: SocketAddr = "127.0.0.1:9".parse().unwrap();

    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: SUPPORTED_VERSION,
        feature_bits: REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let chain = Arc::new(Mutex::new(Blockchain::default()));
    peers.handle_message(
        Message::new(Payload::Handshake(hello), &sk),
        Some(addr),
        &chain,
    );

    let chunk = BlobChunk {
        root: [1u8; 32],
        index: 0,
        total: 1,
        data: vec![0; 256],
    };
    peers.handle_message(
        Message::new(Payload::BlobChunk(chunk.clone()), &sk),
        Some(addr),
        &chain,
    );
    let db = SimpleDb::open(dir.path().join("chunks").to_str().unwrap());
    assert_eq!(db.keys_with_prefix("chunk/").len(), 1);

    let chunk2 = BlobChunk {
        root: [1u8; 32],
        index: 1,
        total: 2,
        data: vec![0; 400],
    };
    peers.handle_message(
        Message::new(Payload::BlobChunk(chunk2), &sk),
        Some(addr),
        &chain,
    );
    assert!(peers.list().is_empty());
    let db2 = SimpleDb::open(dir.path().join("chunks").to_str().unwrap());
    assert_eq!(db2.keys_with_prefix("chunk/").len(), 1);
}
