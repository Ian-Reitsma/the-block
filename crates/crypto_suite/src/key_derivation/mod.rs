pub mod hkdf;
pub mod inhouse;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyDerivationError {
    #[error("invalid context string")]
    InvalidContext,
    #[error("derivation failed")]
    DerivationFailed,
}

pub trait KeyDeriver {
    fn derive_key(&self, context: &[u8], material: &[u8]) -> Result<[u8; 32], KeyDerivationError>;
}
