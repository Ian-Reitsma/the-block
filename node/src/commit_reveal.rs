#[cfg(feature = "pq-crypto")]
use concurrency::Lazy;
#[cfg(not(feature = "pq-crypto"))]
use crypto_suite::hashing::blake3;
#[cfg(feature = "pq-crypto")]
use pqcrypto_dilithium::dilithium3::{
    detached_sign, keypair, verify_detached_signature, DetachedSignature, PublicKey, SecretKey,
};

#[cfg(feature = "pq-crypto")]
static KEYPAIR: Lazy<(PublicKey, SecretKey)> = Lazy::new(|| keypair());

#[cfg(feature = "pq-crypto")]
pub fn commit(salt: &[u8], state: &[u8], nonce: u64) -> (Vec<u8>, u64) {
    let mut msg = Vec::with_capacity(salt.len() + state.len() + 8);
    msg.extend_from_slice(salt);
    msg.extend_from_slice(&nonce.to_le_bytes());
    msg.extend_from_slice(state);
    let sig = detached_sign(&msg, &KEYPAIR.1).as_bytes().to_vec();
    (sig, nonce)
}

#[cfg(feature = "pq-crypto")]
pub fn verify(salt: &[u8], state: &[u8], sig: &[u8], nonce: u64) -> bool {
    let mut msg = Vec::with_capacity(salt.len() + state.len() + 8);
    msg.extend_from_slice(salt);
    msg.extend_from_slice(&nonce.to_le_bytes());
    msg.extend_from_slice(state);
    if let Ok(ds) = DetachedSignature::from_bytes(sig) {
        verify_detached_signature(&ds, &msg, &KEYPAIR.0).is_ok()
    } else {
        false
    }
}

#[cfg(not(feature = "pq-crypto"))]
pub fn commit(salt: &[u8], state: &[u8], nonce: u64) -> (Vec<u8>, u64) {
    let mut hasher = blake3::Hasher::new();
    hasher.update(salt);
    hasher.update(&nonce.to_le_bytes());
    hasher.update(state);
    (hasher.finalize().as_bytes().to_vec(), nonce)
}

#[cfg(not(feature = "pq-crypto"))]
pub fn verify(salt: &[u8], state: &[u8], sig: &[u8], nonce: u64) -> bool {
    commit(salt, state, nonce).0 == sig
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let salt = b"s";
        let state = b"payload";
        let (sig, nonce) = commit(salt, state, 1);
        #[cfg(feature = "pq-crypto")]
        assert!(sig.len() > 0);
        #[cfg(not(feature = "pq-crypto"))]
        assert_eq!(sig.len(), 32);
        assert!(verify(salt, state, &sig, nonce));
    }
}
