#![cfg(feature = "quic")]

use ed25519_dalek::SigningKey;
use tempfile::tempdir;
use the_block::net::{
    record_peer_certificate, transport_quic, verify_peer_fingerprint,
};

#[test]
fn rotates_and_tracks_fingerprints() {
    let dir = tempdir().expect("tempdir");
    let cert_store = dir.path().join("certs.json");
    let peer_store = dir.path().join("peers.json");
    std::env::set_var("TB_NET_CERT_STORE_PATH", &cert_store);
    std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);

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
        rotated.cert.clone(),
        rotated.fingerprint,
        rotated.previous.clone(),
    );
    assert!(verify_peer_fingerprint(&peer, Some(&rotated.fingerprint)));
}
