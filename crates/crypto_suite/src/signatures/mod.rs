pub mod ed25519;
mod ed25519_inhouse;

#[doc(hidden)]
pub mod internal {
    pub use super::ed25519_inhouse::Sha512;
}

/// Trait implemented by signing keys capable of producing detached signatures.
pub trait Signer {
    type Signature;

    fn sign(&self, message: &[u8]) -> Self::Signature;
}

/// Trait implemented by public keys that can verify detached signatures.
pub trait Verifier<S> {
    type Error;

    fn verify(&self, message: &[u8], signature: &S) -> Result<(), Self::Error>;
}
