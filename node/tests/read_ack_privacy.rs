#![cfg(feature = "integration-tests")]

use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::OsRng;
use the_block::ad_readiness::{AdReadinessConfig, AdReadinessHandle};
use the_block::config::ReadAckPrivacyMode;
use the_block::{Blockchain, ReadAck, ReadAckError};

fn build_ack(bytes: u64, domain: &str, provider: &str) -> ReadAck {
    let mut rng = OsRng::default();
    let signing = SigningKey::generate(&mut rng);
    let verifying = signing.verifying_key();
    let manifest = [0xAA; 32];
    let path_hash = [0xBB; 32];
    let ts = 1_701_000_000u64;
    let client_hash = [0x11; 32];

    let mut hasher = Hasher::new();
    hasher.update(&manifest);
    hasher.update(&path_hash);
    hasher.update(&bytes.to_le_bytes());
    hasher.update(&ts.to_le_bytes());
    hasher.update(&client_hash);
    let message = hasher.finalize();
    let signature = signing.sign(message.as_bytes());

    ReadAck {
        manifest,
        path_hash,
        bytes,
        ts,
        client_hash,
        pk: verifying.to_bytes(),
        sig: signature.to_bytes().to_vec(),
        domain: domain.to_string(),
        provider: provider.to_string(),
        campaign_id: None,
        creative_id: None,
        readiness: None,
        zk_proof: None,
    }
}

#[test]
fn privacy_proof_verifies_and_detects_tampering() {
    let mut ack = build_ack(512, "example.org", "edge-01");
    assert!(ack.verify());
    let readiness = AdReadinessHandle::new(AdReadinessConfig::default());
    let snapshot = readiness.snapshot();
    ack.attach_privacy(snapshot.clone());
    assert!(ack.verify_privacy());

    let mut tampered = ack.clone();
    tampered.domain = "evil.example".into();
    assert!(!tampered.verify_privacy());

    let mut mismatched = ack.clone();
    mismatched
        .readiness
        .as_mut()
        .and_then(|snap| snap.zk_proof.as_mut())
        .map(|proof| proof.blinding_mut()[0] ^= 0xFF);
    assert!(!mismatched.verify_privacy());
}

#[test]
fn reservation_discriminator_differs_per_signature() {
    let ack_a = build_ack(256, "example.org", "edge-01");
    let ack_b = build_ack(256, "example.org", "edge-01");
    assert_ne!(
        ack_a.reservation_discriminator(),
        ack_b.reservation_discriminator()
    );
}

fn tampered_ack() -> (ReadAck, ReadAck) {
    let mut ack = build_ack(512, "example.org", "edge-01");
    let readiness = AdReadinessHandle::new(AdReadinessConfig::default());
    let snapshot = readiness.snapshot();
    ack.attach_privacy(snapshot);
    let mut tampered = ack.clone();
    tampered.domain = "other.example".into();
    (ack, tampered)
}

#[test]
fn enforce_mode_rejects_invalid_privacy() {
    let mut chain = Blockchain::default();
    chain.config.read_ack_privacy = ReadAckPrivacyMode::Enforce;
    let (valid, invalid) = tampered_ack();
    assert!(chain.submit_read_ack(valid).is_ok());
    let err = chain
        .submit_read_ack(invalid.clone())
        .expect_err("should reject");
    assert_eq!(err, ReadAckError::PrivacyProofRejected);
    // signature remains valid even though privacy proof fails
    assert!(invalid.verify_signature());
}

#[test]
fn observe_mode_allows_invalid_privacy() {
    let mut chain = Blockchain::default();
    chain.config.read_ack_privacy = ReadAckPrivacyMode::Observe;
    let (_, invalid) = tampered_ack();
    assert!(chain.submit_read_ack(invalid).is_ok());
}

#[test]
fn disabled_mode_skips_privacy() {
    let mut chain = Blockchain::default();
    chain.config.read_ack_privacy = ReadAckPrivacyMode::Disabled;
    let (_, invalid) = tampered_ack();
    assert!(chain.submit_read_ack(invalid).is_ok());
}
