use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;

/// Generate a pseudo master seed for HD wallets.
pub fn generate_master() -> [u8; 32] {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    seed
}

/// Derived signing/verifying key material for a pseudo HD path.
/// This is a placeholder and does not implement full BIP32 semantics.
#[derive(Clone)]
pub struct Keypair {
    pub secret: SigningKey,
    pub public: VerifyingKey,
}

pub fn derive_child(master: &[u8; 32], _path: &str) -> Keypair {
    let secret = SigningKey::from_bytes(master);
    let public = secret.verifying_key();
    Keypair { secret, public }
}
