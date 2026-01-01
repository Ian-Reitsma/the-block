#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::binary;
use std::sync::{Arc, Mutex};
use sys::tempfile::tempdir;
use testkit::tb_prop_test;
use the_block::{
    net::{self, Message, Payload, PeerSet},
    p2p::handshake::{Hello, Transport},
    Blockchain,
};

fn sample_sk() -> SigningKey {
    SigningKey::from_bytes(&[0u8; 32])
}

tb_prop_test!(fuzz_identifier_exchange, |runner| {
    runner
        .add_case("default handshake", || {
            let dir = tempdir().unwrap();
            net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
            std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
            let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
            let peers = PeerSet::new(vec![]);
            let hello = Hello {
                network_id: [0u8; 4],
                proto_version: 0,
                feature_bits: 0,
                agent: String::new(),
                nonce: 0,
                transport: Transport::Tcp,
                gossip_addr: None,
                quic_addr: None,
                quic_cert: None,
                quic_fingerprint: None,
                quic_fingerprint_previous: Vec::new(),
                quic_provider: None,
                quic_capabilities: Vec::new(),
            };
            let msg =
                Message::new(Payload::Handshake(hello), &sample_sk()).expect("sign handshake");
            peers.handle_message(msg, None, &bc);
        })
        .expect("register case");

    runner
        .add_random_case("handshake permutations", 64, |rng| {
            let proto_version = rng.range_u16(0..=u16::MAX);
            let feature_bits = rng.range_u32(0..=u32::MAX);
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
                gossip_addr: None,
                quic_addr: None,
                quic_cert: None,
                quic_fingerprint: None,
                quic_fingerprint_previous: Vec::new(),
                quic_provider: None,
                quic_capabilities: Vec::new(),
            };
            let msg =
                Message::new(Payload::Handshake(hello), &sample_sk()).expect("sign handshake");
            peers.handle_message(msg, None, &bc);
        })
        .expect("register random case");
});

tb_prop_test!(fuzz_malformed_handshake, |runner| {
    runner
        .add_random_case("deserialize fuzz", 96, |rng| {
            let raw = rng.bytes(0..=256);
            let dir = tempdir().unwrap();
            net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
            std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
            let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
            let peers = PeerSet::new(vec![]);
            if let Ok(msg) = binary::decode::<Message>(&raw) {
                peers.handle_message(msg, None, &bc);
            }
        })
        .expect("register random case");
});
