//! Dilithium key generation, signing, and verification utilities.
//! Guarded behind the `quantum` feature flag.

#[cfg(feature = "quantum")]
use pqcrypto_dilithium::dilithium3;
#[cfg(feature = "quantum")]
/// Generate a Dilithium keypair returning `(public_key, secret_key)` byte vectors.
pub fn keypair() -> (Vec<u8>, Vec<u8>) {
    let (pk, sk) = dilithium3::keypair();
    (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
}

#[cfg(feature = "quantum")]
/// Sign `msg` with a Dilithium secret key.
pub fn sign(sk: &[u8], msg: &[u8]) -> Vec<u8> {
    let sk = dilithium3::SecretKey::from_bytes(sk).expect("sk length");
    let sig = dilithium3::detached_sign(msg, &sk);
    sig.as_bytes().to_vec()
}

#[cfg(feature = "quantum")]
/// Verify a Dilithium signature. Returns `true` on success.
pub fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    if let (Ok(pk), Ok(sig)) = (
        dilithium3::PublicKey::from_bytes(pk),
        dilithium3::DetachedSignature::from_bytes(sig),
    ) {
        dilithium3::verify_detached_signature(&sig, msg, &pk).is_ok()
    } else {
        false
    }
}
