#![cfg(all(feature = "integration-tests", feature = "quic"))]

use crypto_suite::signatures::ed25519::SigningKey;
use runtime;
use std::time::Duration;
use tempfile::tempdir;
use the_block::net::{
    peer_cert_history, record_peer_certificate, refresh_peer_cert_store_from_disk, transport_quic,
};
use transport::{Config as TransportConfig, ProviderKind};

fn setup_env(dir: &tempfile::TempDir) {
    let cert_store = dir.path().join("peer_certs.json");
    let net_key = dir.path().join("net_key");
    let quic_store = dir.path().join("quic_local.json");
    std::env::set_var("TB_PEER_CERT_CACHE_PATH", &cert_store);
    std::env::set_var("TB_NET_KEY_PATH", &net_key);
    std::env::set_var("TB_NET_CERT_STORE_PATH", &quic_store);
    // ensure the in-memory cache points at the new path
    let _ = refresh_peer_cert_store_from_disk();
    let mut cfg = TransportConfig::default();
    cfg.provider = ProviderKind::S2nQuic;
    cfg.certificate_cache = Some(quic_store);
    the_block::net::configure_transport(&cfg).expect("configure transport");
}

fn teardown_env() {
    std::env::remove_var("TB_PEER_CERT_CACHE_PATH");
    std::env::remove_var("TB_NET_KEY_PATH");
    std::env::remove_var("TB_NET_CERT_STORE_PATH");
}

#[testkit::tb_serial]
fn encrypts_and_reloads_quic_peer_certs() {
    let dir = tempdir().expect("tempdir");
    setup_env(&dir);

    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let advert = transport_quic::initialize(&signing_key).expect("init cert");

    let peer = [11u8; 32];
    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        advert.cert.clone(),
        advert.fingerprint,
        advert.previous.clone(),
    );

    let store_path = dir.path().join("peer_certs.json");
    let contents = std::fs::read_to_string(&store_path).expect("read store");
    assert!(contents.contains("enc:v1:"));

    let history = peer_cert_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].peer, hex::encode(peer));
    assert_eq!(
        history[0].current.fingerprint,
        hex::encode(advert.fingerprint)
    );

    std::fs::write(&store_path, "[]").expect("truncate store");
    runtime::block_on(async {
        for _ in 0..40 {
            if peer_cert_history().is_empty() {
                return;
            }
            runtime::sleep(Duration::from_millis(50)).await;
        }
        panic!("peer cert watcher did not observe external change");
    });

    teardown_env();
}

#[testkit::tb_serial]
fn prunes_stale_quic_cert_history() {
    let dir = tempdir().expect("tempdir");
    setup_env(&dir);

    let signing_key = SigningKey::from_bytes(&[13u8; 32]);
    let advert = transport_quic::initialize(&signing_key).expect("init cert");
    let rotated = transport_quic::rotate(&signing_key).expect("rotate cert");

    let peer = [29u8; 32];
    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        advert.cert.clone(),
        advert.fingerprint,
        advert.previous.clone(),
    );
    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        rotated.cert.clone(),
        rotated.fingerprint,
        rotated.previous.clone(),
    );

    let store_path = dir.path().join("peer_certs.json");
    let mut data = std::fs::read_to_string(&store_path).expect("read store");
    let mut json: foundation_serialization::json::Value =
        foundation_serialization::json::from_str(&data).expect("decode store");
    if let Some(array) = json.as_array_mut() {
        if let Some(entry) = array.first_mut() {
            if let Some(history) = entry.get_mut("history") {
                if let Some(first) = history.as_array_mut().and_then(|v| v.first_mut()) {
                    first["updated_at"] = foundation_serialization::json::json!(0);
                }
            }
        }
    }
    data = foundation_serialization::json::to_string_pretty(&json).expect("encode store");
    std::fs::write(&store_path, data).expect("write store");
    let _ = refresh_peer_cert_store_from_disk();

    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        rotated.cert.clone(),
        rotated.fingerprint,
        rotated.previous.clone(),
    );

    let history = peer_cert_history();
    assert_eq!(history.len(), 1);
    assert!(history[0].history.is_empty());

    teardown_env();
}

#[testkit::tb_serial]
fn refresh_clears_removed_quic_cert_store() {
    let dir = tempdir().expect("tempdir");
    setup_env(&dir);

    let signing_key = SigningKey::from_bytes(&[55u8; 32]);
    let advert = transport_quic::initialize(&signing_key).expect("init cert");
    let peer = [201u8; 32];
    record_peer_certificate(
        &peer,
        transport::ProviderKind::S2nQuic.id(),
        advert.cert.clone(),
        advert.fingerprint,
        advert.previous.clone(),
    );

    let store_path = dir.path().join("peer_certs.json");
    std::fs::write(&store_path, "[]").expect("truncate store");
    let _ = refresh_peer_cert_store_from_disk();

    let history = peer_cert_history();
    assert!(history.is_empty());

    teardown_env();
}
