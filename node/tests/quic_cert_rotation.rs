#![cfg(feature = "integration-tests")]
#![cfg(feature = "quic")]

use crypto_suite::signatures::ed25519::SigningKey;
use tempfile::tempdir;
use the_block::net::{
    record_peer_certificate, transport_quic, verify_peer_fingerprint, HandshakeError, Hello,
    Transport, SUPPORTED_VERSION,
};
use the_block::p2p::handshake::validate_quic_certificate;
use transport::{Config as TransportConfig, ProviderKind};

fn configure_transport(quic_store: &std::path::Path) {
    let mut cfg = TransportConfig::default();
    cfg.provider = ProviderKind::S2nQuic;
    cfg.certificate_cache = Some(quic_store.to_path_buf());
    the_block::net::configure_transport(&cfg).expect("configure transport");
}

#[test]
fn rotates_and_tracks_fingerprints() {
    let dir = tempdir().expect("tempdir");
    let cert_store = dir.path().join("certs.json");
    let peer_store = dir.path().join("peers.json");
    std::env::set_var("TB_NET_CERT_STORE_PATH", &cert_store);
    std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);
    configure_transport(&cert_store);

    let key_bytes = [7u8; 32];
    let signing_key = SigningKey::from_bytes(&key_bytes);

    let first = transport_quic::initialize(&signing_key).expect("init cert");
    let first_fp = first.fingerprint;
    let rotated = transport_quic::rotate(&signing_key).expect("rotate cert");
    let rotated_fp = rotated.fingerprint;
    assert_ne!(first_fp, rotated_fp);

    let history = transport_quic::fingerprint_history();
    assert!(history.contains(&first_fp));
    assert!(history.contains(&rotated_fp));

    let peer = [3u8; 32];
    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        rotated.cert.clone(),
        rotated.fingerprint,
        rotated.previous.clone(),
    );
    assert!(verify_peer_fingerprint(&peer, Some(&rotated.fingerprint)));
}

#[test]
fn fingerprint_mismatch_rejects_certificate() {
    let dir = tempdir().expect("tempdir");
    let cert_store = dir.path().join("certs.json");
    let peer_store = dir.path().join("peers.json");
    std::env::set_var("TB_NET_CERT_STORE_PATH", &cert_store);
    std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);
    configure_transport(&cert_store);

    let peer_key = [9u8; 32];
    let signing_key = SigningKey::from_bytes(&peer_key);
    let advert = transport_quic::initialize(&signing_key).expect("init cert");

    let mut forged_fp = advert.fingerprint;
    forged_fp[0] ^= 0xFF;

    let hello = Hello {
        network_id: [0; 4],
        proto_version: SUPPORTED_VERSION,
        feature_bits: 0,
        agent: "test/1.0".into(),
        nonce: 0,
        transport: Transport::Quic,
        quic_addr: None,
        quic_cert: Some(advert.cert.clone()),
        quic_fingerprint: Some(forged_fp.to_vec()),
        quic_fingerprint_previous: Vec::new(),
        quic_provider: Some(transport::ProviderKind::S2nQuic.id().to_string()),
        quic_capabilities: vec!["certificate_rotation".into()],
    };

    let result = validate_quic_certificate(&peer_key, &hello);
    match result {
        Err(HandshakeError::Certificate) => {}
        other => panic!("unexpected result: {:?}", other),
    }
}
