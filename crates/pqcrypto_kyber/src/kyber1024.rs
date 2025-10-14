use crate::common::{
    self, Ciphertext as CiphertextImpl, Error, PublicKey as PublicKeyImpl,
    SecretKey as SecretKeyImpl, SharedSecret as SharedSecretImpl,
};

pub const PUBLIC_KEY_BYTES: usize = 1568;
pub const SECRET_KEY_BYTES: usize = 3168;
pub const CIPHERTEXT_BYTES: usize = 1568;
pub const SHARED_SECRET_BYTES: usize = 32;

pub type PublicKey = PublicKeyImpl<PUBLIC_KEY_BYTES>;
pub type SecretKey = SecretKeyImpl<SECRET_KEY_BYTES, PUBLIC_KEY_BYTES>;
pub type Ciphertext = CiphertextImpl<CIPHERTEXT_BYTES>;
pub type SharedSecret = SharedSecretImpl<SHARED_SECRET_BYTES>;

pub fn keypair() -> (PublicKey, SecretKey) {
    common::keypair::<PUBLIC_KEY_BYTES, SECRET_KEY_BYTES>()
}

pub fn encapsulate(public: &PublicKey) -> (Ciphertext, SharedSecret) {
    common::encapsulate::<CIPHERTEXT_BYTES, SHARED_SECRET_BYTES, PUBLIC_KEY_BYTES>(public)
}

pub fn secret_key_from_bytes(bytes: &[u8]) -> Result<SecretKey, Error> {
    SecretKey::from_bytes(bytes)
}

pub fn public_key_from_bytes(bytes: &[u8]) -> Result<PublicKey, Error> {
    PublicKey::from_bytes(bytes)
}
