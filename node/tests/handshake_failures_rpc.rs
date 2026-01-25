#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use std::sync::{Arc, Mutex};
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::net::{self, Hello, Message, Payload, PROTOCOL_VERSION};
use the_block::p2p::handshake::Transport;
use the_block::{generate_keypair, Blockchain};

fn init_env() -> sys::tempfile::TempDir {
    let dir = sys::tempfile::tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("http_proxy");
    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("https_proxy");
    dir
}

#[testkit::tb_serial]
fn rpc_reports_handshake_failures() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = net::PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
        let pk: [u8; 32] = pk_vec.as_slice().try_into().unwrap();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: net::REQUIRED_FEATURES,
            agent: "test".into(),
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
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign handshake");
        peers.handle_message(msg, None, &bc);

        // simulate failure and expose via RPC
        net::simulate_handshake_fail(pk, net::HandshakeError::Tls);
        // Ensure the failure is recorded and exposed.
        let failures = net::recent_handshake_failures();
        assert!(
            !failures.is_empty(),
            "expected handshake failures to be recorded"
        );
        let expected_peer = net::overlay_peer_from_bytes(&pk)
            .map(|p| net::overlay_peer_to_base58(&p))
            .unwrap_or_else(|_| crypto_suite::hex::encode(pk));
        assert!(
            failures.iter().any(|(_, peer, _)| peer == &expected_peer),
            "expected failure entry for peer {expected_peer}"
        );
        Settlement::shutdown();
    });
}
