use ed25519_dalek::{Keypair, PublicKey};
use rand::rngs::OsRng;
use rand::RngCore;

/// Generate a pseudo master seed for HD wallets.
pub fn generate_master() -> [u8; 32] {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    seed
}

/// Derive a child keypair from the master seed and a derivation path.
/// This is a placeholder and does not implement full BIP32 semantics.
pub fn derive_child(master: &[u8; 32], _path: &str) -> Keypair {
    let secret = ed25519_dalek::SecretKey::from_bytes(master).expect("seed");
    let public = PublicKey::from(&secret);
    Keypair { secret, public }
}
