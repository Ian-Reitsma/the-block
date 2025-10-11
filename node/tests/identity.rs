#![cfg(feature = "integration-tests")]
use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_unicode::{NormalizationAccuracy, Normalizer};
use sys::tempfile::tempdir;
use the_block::generate_keypair;
use the_block::identity::handle_registry::{HandleError, HandleRegistry};

fn sign_msg(handle: &str, sk: &SigningKey, nonce: u64) -> (Vec<u8>, Vec<u8>) {
    let normalizer = Normalizer::default();
    let handle_norm = normalizer.nfkc(handle).as_str().to_lowercase();
    let pk = sk.verifying_key();
    let mut h = blake3::Hasher::new();
    h.update(b"register:");
    h.update(handle_norm.as_bytes());
    h.update(&pk.to_bytes());
    h.update(&nonce.to_le_bytes());
    let msg = h.finalize();
    let sig = sk.sign(msg.as_bytes());
    (pk.to_bytes().to_vec(), sig.to_bytes().to_vec())
}

#[test]
fn register_persists() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let (pk, sig) = sign_msg("alice", &sk, 1);
    {
        let mut reg = HandleRegistry::open(path);
        let outcome = reg.register_handle("alice", &pk, &sig, 1).unwrap();
        assert_eq!(outcome.normalized_handle, "alice");
        assert_eq!(outcome.accuracy, NormalizationAccuracy::Exact);
    }
    {
        let reg = HandleRegistry::open(path);
        assert_eq!(
            reg.resolve_handle("alice").unwrap(),
            crypto_suite::hex::encode(pk)
        );
    }
}

#[test]
fn duplicate_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (sk1_bytes, _) = generate_keypair();
    let sk1 = SigningKey::from_bytes(&sk1_bytes.try_into().unwrap());
    let (sk2_bytes, _) = generate_keypair();
    let sk2 = SigningKey::from_bytes(&sk2_bytes.try_into().unwrap());
    let (pk1, sig1) = sign_msg("bob", &sk1, 1);
    let (pk2, sig2) = sign_msg("bob", &sk2, 1);
    let mut reg = HandleRegistry::open(path);
    reg.register_handle("bob", &pk1, &sig1, 1).unwrap();
    let err = reg
        .register_handle("bob", &pk2, &sig2, 1)
        .err()
        .expect("duplicate registration should fail");
    assert!(matches!(err, HandleError::Duplicate));
}

#[test]
fn replay_and_higher_nonce() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let (pk, sig1) = sign_msg("carol", &sk, 1);
    let mut reg = HandleRegistry::open(path);
    reg.register_handle("carol", &pk, &sig1, 1).unwrap();
    let (_, sig1r) = sign_msg("carol", &sk, 1);
    assert!(matches!(
        reg.register_handle("carol", &pk, &sig1r, 1),
        Err(HandleError::LowNonce)
    ));
    let (_, sig2) = sign_msg("carol", &sk, 2);
    let outcome = reg.register_handle("carol", &pk, &sig2, 2).unwrap();
    assert_eq!(outcome.normalized_handle, "carol");
    assert_eq!(outcome.accuracy, NormalizationAccuracy::Exact);
}

#[test]
fn reserved_and_case_conflict() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let (pk, sig) = sign_msg("Sys/Admin", &sk, 1);
    let mut reg = HandleRegistry::open(path);
    assert!(matches!(
        reg.register_handle("sys/root", &pk, &sig, 1),
        Err(HandleError::Reserved)
    ));
    let (pk2, sig2) = sign_msg("Alice", &sk, 2);
    let outcome = reg.register_handle("Alice", &pk2, &sig2, 2).unwrap();
    assert_eq!(outcome.normalized_handle, "alice");
    assert_eq!(outcome.accuracy, NormalizationAccuracy::Exact);
    assert!(reg.resolve_handle("alice").is_some());
}

#[test]
fn approximate_handles_report_accuracy() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let (pk, sig) = sign_msg("École", &sk, 1);
    let mut reg = HandleRegistry::open(path);
    let outcome = reg.register_handle("École", &pk, &sig, 1).unwrap();
    assert_eq!(outcome.normalized_handle, "ecole");
    assert_eq!(outcome.accuracy, NormalizationAccuracy::Approximate);
}
