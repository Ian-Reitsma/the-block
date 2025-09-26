use core::fmt;
use core::hash::{Hash, Hasher};

use ed25519_dalek::{self, pkcs8, Signer as DalekSigner, Verifier as DalekVerifier};
use rand_core::{CryptoRng, RngCore};
use thiserror::Error;

use crate::signatures::{Signer, Verifier};

#[cfg(feature = "telemetry")]
use std::sync::OnceLock;

pub const ALGORITHM: &str = "ed25519";
pub const BACKEND: &str = "ed25519-dalek";
pub const BACKEND_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "telemetry")]
type TelemetryHook = fn(&'static str, &'static str, &'static str, bool);

#[cfg(feature = "telemetry")]
static TELEMETRY_HOOK: OnceLock<TelemetryHook> = OnceLock::new();

#[cfg(feature = "telemetry")]
#[derive(Debug, Error)]
pub enum TelemetryHookError {
    #[error("crypto suite telemetry hook already installed")]
    AlreadyInstalled,
}

#[cfg(feature = "telemetry")]
pub fn install_telemetry_hook(hook: TelemetryHook) -> Result<(), TelemetryHookError> {
    TELEMETRY_HOOK
        .set(hook)
        .map_err(|_| TelemetryHookError::AlreadyInstalled)
}

#[cfg(feature = "telemetry")]
fn record(operation: &'static str, success: bool) {
    if let Some(hook) = TELEMETRY_HOOK.get() {
        hook(ALGORITHM, operation, BACKEND, success);
    }
}

pub const PUBLIC_KEY_LENGTH: usize = ed25519_dalek::PUBLIC_KEY_LENGTH;
pub const SECRET_KEY_LENGTH: usize = ed25519_dalek::SECRET_KEY_LENGTH;
pub const KEYPAIR_LENGTH: usize = ed25519_dalek::KEYPAIR_LENGTH;
pub const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;

#[derive(Clone)]
pub struct SigningKey(ed25519_dalek::SigningKey);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(ed25519_dalek::Signature);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyingKey(ed25519_dalek::VerifyingKey);

#[derive(Debug, Error)]
pub enum KeyEncodingError {
    #[error("pkcs8 encoding failed: {0}")]
    Encoding(pkcs8::Error),
    #[error("pkcs8 decoding failed: {0}")]
    Decoding(pkcs8::Error),
}

pub type SignatureError = ed25519_dalek::SignatureError;

/// Legacy type aliases retained while downstream crates migrate off the
/// previous `crypto::ed25519::*` exports.
pub mod legacy {
    pub type Signature = super::Signature;
    pub type SigningKey = super::SigningKey;
    pub type VerifyingKey = super::VerifyingKey;
    pub type SignatureError = super::SignatureError;
}

impl SigningKey {
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        Self(ed25519_dalek::SigningKey::generate(rng))
    }

    pub fn from_bytes(bytes: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self(ed25519_dalek::SigningKey::from_bytes(bytes))
    }

    pub fn from_keypair_bytes(bytes: &[u8; KEYPAIR_LENGTH]) -> Result<Self, SignatureError> {
        ed25519_dalek::SigningKey::from_keypair_bytes(bytes).map(Self)
    }

    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.0.to_bytes()
    }

    pub fn to_keypair_bytes(&self) -> [u8; KEYPAIR_LENGTH] {
        self.0.to_keypair_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.0.verifying_key())
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = Signature(DalekSigner::sign(&self.0, message));
        #[cfg(feature = "telemetry")]
        record("sign", true);
        sig
    }

    pub fn to_pkcs8_der(&self) -> Result<pkcs8::SecretDocument, KeyEncodingError> {
        use ed25519_dalek::pkcs8::EncodePrivateKey;
        self.0.to_pkcs8_der().map_err(KeyEncodingError::Encoding)
    }

    pub fn from_pkcs8_der(bytes: &[u8]) -> Result<Self, KeyEncodingError> {
        use ed25519_dalek::pkcs8::DecodePrivateKey;
        ed25519_dalek::SigningKey::from_pkcs8_der(bytes)
            .map(Self)
            .map_err(KeyEncodingError::Decoding)
    }

    pub fn secret_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.to_bytes()
    }
}

impl Signer for SigningKey {
    type Signature = Signature;

    fn sign(&self, message: &[u8]) -> Self::Signature {
        SigningKey::sign(self, message)
    }
}

impl Signature {
    pub fn from_bytes(bytes: &[u8; SIGNATURE_LENGTH]) -> Self {
        Self(ed25519_dalek::Signature::from_bytes(bytes))
    }

    pub fn to_bytes(&self) -> [u8; SIGNATURE_LENGTH] {
        self.0.to_bytes()
    }

    pub fn as_bytes(&self) -> [u8; SIGNATURE_LENGTH] {
        self.0.to_bytes()
    }
}

impl From<ed25519_dalek::Signature> for Signature {
    fn from(value: ed25519_dalek::Signature) -> Self {
        Self(value)
    }
}

impl From<Signature> for [u8; SIGNATURE_LENGTH] {
    fn from(value: Signature) -> Self {
        value.to_bytes()
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey").finish_non_exhaustive()
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature")
            .field(&hex::encode(self.0.to_bytes()))
            .finish()
    }
}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bytes = self.0.to_bytes();
        state.write(&bytes);
    }
}

impl Hash for VerifyingKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bytes = self.0.to_bytes();
        state.write(&bytes);
    }
}

impl VerifyingKey {
    pub fn from_bytes(bytes: &[u8; PUBLIC_KEY_LENGTH]) -> Result<Self, SignatureError> {
        ed25519_dalek::VerifyingKey::from_bytes(bytes).map(Self)
    }

    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.0.to_bytes()
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), SignatureError> {
        let result = DalekVerifier::verify(&self.0, message, &signature.0);
        #[cfg(feature = "telemetry")]
        record("verify", result.is_ok());
        result
    }

    pub fn verify_strict(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), SignatureError> {
        let result = self.0.verify_strict(message, &signature.0);
        #[cfg(feature = "telemetry")]
        record("verify_strict", result.is_ok());
        result
    }
}

impl Verifier<Signature> for VerifyingKey {
    type Error = SignatureError;

    fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), Self::Error> {
        VerifyingKey::verify(self, message, signature)
    }
}

mod serde_impls {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for Signature {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let bytes = self.to_bytes();
            serializer.serialize_bytes(&bytes)
        }
    }

    impl<'de> Deserialize<'de> for Signature {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
            if bytes.len() != SIGNATURE_LENGTH {
                return Err(serde::de::Error::invalid_length(bytes.len(), &"64"));
            }
            let mut arr = [0u8; SIGNATURE_LENGTH];
            arr.copy_from_slice(&bytes);
            Ok(Signature::from_bytes(&arr))
        }
    }

    impl Serialize for VerifyingKey {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_bytes(&self.to_bytes())
        }
    }

    impl<'de> Deserialize<'de> for VerifyingKey {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
            if bytes.len() != PUBLIC_KEY_LENGTH {
                return Err(serde::de::Error::invalid_length(bytes.len(), &"32"));
            }
            let mut arr = [0u8; PUBLIC_KEY_LENGTH];
            arr.copy_from_slice(&bytes);
            VerifyingKey::from_bytes(&arr).map_err(|e| serde::de::Error::custom(e.to_string()))
        }
    }
}
