#![cfg(feature = "integration-tests")]

use governance_spec::{decode_transport_provider_policy, DEFAULT_TRANSPORT_PROVIDER_POLICY};
use serial_test::serial;
use the_block::config::{
    self, current, set_current, set_storage_engine_policy, set_transport_provider_policy,
    NodeConfig, OverlayBackend,
};
use the_block::simple_db::EngineKind;

#[test]
#[serial]
fn storage_policy_enforces_fallbacks() {
    let original = current();

    let mut cfg = NodeConfig::default();
    cfg.overlay.backend = OverlayBackend::Stub;
    cfg.storage.default_engine = EngineKind::Memory;
    cfg.storage
        .overrides
        .insert("bridge".into(), EngineKind::Memory);

    set_current(cfg.clone());
    set_storage_engine_policy(&["sled".to_string()]);

    let updated = current();
    assert_eq!(updated.storage.default_engine, EngineKind::Sled);
    assert_eq!(
        updated.storage.overrides.get("bridge").copied().unwrap(),
        EngineKind::Sled
    );

    set_current(original.clone());
    set_storage_engine_policy(&["sled".to_string()]);
}

#[test]
#[serial]
fn transport_policy_updates_provider() {
    let original = current();

    let mut cfg = NodeConfig::default();
    cfg.overlay.backend = OverlayBackend::Stub;
    let mut quic = config::QuicConfig::default();
    quic.transport.provider = Some("s2n-quic".into());
    cfg.quic = Some(quic);

    set_current(cfg.clone());
    set_transport_provider_policy(&["quinn".to_string()]);

    let updated = current();
    let provider = updated
        .quic
        .as_ref()
        .and_then(|q| q.transport.provider.as_deref())
        .map(|s| s.to_string());
    assert_eq!(provider.as_deref(), Some("quinn"));

    set_current(original.clone());
    let reset = decode_transport_provider_policy(DEFAULT_TRANSPORT_PROVIDER_POLICY);
    set_transport_provider_policy(&reset);
}
