#![cfg(feature = "integration-tests")]
#![cfg(feature = "pq-crypto")]
use crypto_suite::hashing::blake3;
use pqcrypto_dilithium::dilithium2;
use the_block::identity::handle_registry::HandleRegistry;

#[test]
fn pq_key_stored() {
    let kp = dilithium2::keypair();
    let pq_pk = kp.0.as_bytes().to_vec();
    let dir = sys::tempfile::tempdir().unwrap();
    let mut reg = HandleRegistry::open(dir.path().join("db").to_str().unwrap());
    // Use dummy ed25519 key for address
    use crypto_suite::signatures::{ed25519::SigningKey, Signer};
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let pk = sk.verifying_key();
    let mut h = blake3::Hasher::new();
    h.update(b"register:");
    h.update(b"test");
    h.update(pk.as_bytes());
    h.update(&1u64.to_le_bytes());
    let msg = h.finalize();
    let sig = sk.sign(msg.as_bytes());
    reg.register_handle("test", pk.as_bytes(), Some(&pq_pk), sig.as_ref(), 1)
        .unwrap();
    assert_eq!(reg.pq_key_of("test").unwrap(), pq_pk);
}
