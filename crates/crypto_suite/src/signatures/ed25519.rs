use core::fmt;
use core::hash::{Hash, Hasher};

use rand::{CryptoRng, RngCore};
use thiserror::Error;

use crate::signatures::{Signer, Verifier};

use super::ed25519_inhouse as backend;

pub use backend::{
    SignatureError, KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};

#[cfg(feature = "telemetry")]
use std::sync::OnceLock;

pub const ALGORITHM: &str = "ed25519";
pub const BACKEND: &str = "inhouse";
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

#[derive(Clone)]
pub struct SigningKey(backend::SigningKey);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(backend::Signature);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyingKey(backend::VerifyingKey);

#[derive(Debug, Error)]
pub enum KeyEncodingError {
    #[error("pkcs8 encoding failed")]
    Encoding,
    #[error("pkcs8 decoding failed")]
    Decoding,
}

#[derive(Clone)]
pub struct SecretDocument {
    bytes: Vec<u8>,
}

impl SecretDocument {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

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
        Self(backend::SigningKey::generate(rng))
    }

    pub fn from_bytes(bytes: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self(backend::SigningKey::from_bytes(bytes))
    }

    pub fn from_keypair_bytes(bytes: &[u8; KEYPAIR_LENGTH]) -> Result<Self, SignatureError> {
        backend::SigningKey::from_keypair_bytes(bytes).map(Self)
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
        let sig = Signature(self.0.sign(message));
        #[cfg(feature = "telemetry")]
        record("sign", true);
        sig
    }

    pub fn to_pkcs8_der(&self) -> Result<SecretDocument, KeyEncodingError> {
        encode_pkcs8(self)
    }

    pub fn from_pkcs8_der(bytes: &[u8]) -> Result<Self, KeyEncodingError> {
        decode_pkcs8(bytes).map(Self)
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
        Self(backend::Signature::from_bytes(bytes))
    }

    pub fn to_bytes(&self) -> [u8; SIGNATURE_LENGTH] {
        self.0.to_bytes()
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
            .field(&crate::hex::encode(self.0.to_bytes()))
            .finish()
    }
}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0.to_bytes());
    }
}

impl Hash for VerifyingKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.0.to_bytes());
    }
}

impl VerifyingKey {
    pub fn from_bytes(bytes: &[u8; PUBLIC_KEY_LENGTH]) -> Result<Self, SignatureError> {
        backend::VerifyingKey::from_bytes(bytes).map(Self)
    }

    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.0.to_bytes()
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), SignatureError> {
        let result = self.0.verify(message, &signature.0);
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

impl From<Signature> for backend::Signature {
    fn from(value: Signature) -> Self {
        value.0
    }
}

impl From<backend::Signature> for Signature {
    fn from(value: backend::Signature) -> Self {
        Signature(value)
    }
}

impl From<VerifyingKey> for backend::VerifyingKey {
    fn from(value: VerifyingKey) -> Self {
        value.0
    }
}

impl From<backend::VerifyingKey> for VerifyingKey {
    fn from(value: backend::VerifyingKey) -> Self {
        VerifyingKey(value)
    }
}

mod serde_impls {
    use super::*;
    use foundation_serialization::{
        de::{self, Deserializer},
        ser::Serializer,
        Deserialize, Serialize,
    };

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
                return Err(de::Error::invalid_length(bytes.len(), &"64"));
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
                return Err(de::Error::invalid_length(bytes.len(), &"32"));
            }
            let mut arr = [0u8; PUBLIC_KEY_LENGTH];
            arr.copy_from_slice(&bytes);
            VerifyingKey::from_bytes(&arr).map_err(|e| de::Error::custom(e.to_string()))
        }
    }
}

const PKCS8_HEADER: [u8; 16] = [
    0x30, 0x53, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

const PKCS8_PUBLIC_PREFIX: [u8; 5] = [0xA1, 0x23, 0x03, 0x21, 0x00];

fn encode_pkcs8(signing_key: &SigningKey) -> Result<SecretDocument, KeyEncodingError> {
    let secret = signing_key.to_bytes();
    let public = signing_key.verifying_key().to_bytes();
    let capacity =
        PKCS8_HEADER.len() + SECRET_KEY_LENGTH + PKCS8_PUBLIC_PREFIX.len() + PUBLIC_KEY_LENGTH;
    let mut encoded = Vec::with_capacity(capacity);
    encoded.extend_from_slice(&PKCS8_HEADER);
    encoded.extend_from_slice(&secret);
    encoded.extend_from_slice(&PKCS8_PUBLIC_PREFIX);
    encoded.extend_from_slice(&public);
    Ok(SecretDocument { bytes: encoded })
}

fn decode_pkcs8(bytes: &[u8]) -> Result<backend::SigningKey, KeyEncodingError> {
    let expected_len =
        PKCS8_HEADER.len() + SECRET_KEY_LENGTH + PKCS8_PUBLIC_PREFIX.len() + PUBLIC_KEY_LENGTH;

    if bytes.len() != expected_len {
        return Err(KeyEncodingError::Decoding);
    }
    if bytes[..PKCS8_HEADER.len()] != PKCS8_HEADER {
        return Err(KeyEncodingError::Decoding);
    }
    let secret_start = PKCS8_HEADER.len();
    let secret_end = secret_start + SECRET_KEY_LENGTH;
    let mut secret = [0u8; SECRET_KEY_LENGTH];
    secret.copy_from_slice(&bytes[secret_start..secret_end]);

    let public_prefix_start = secret_end;
    let public_prefix_end = public_prefix_start + PKCS8_PUBLIC_PREFIX.len();

    if bytes[public_prefix_start..public_prefix_end] != PKCS8_PUBLIC_PREFIX {
        return Err(KeyEncodingError::Decoding);
    }

    let public_start = public_prefix_end;
    let public_end = public_start + PUBLIC_KEY_LENGTH;
    let mut provided_public = [0u8; PUBLIC_KEY_LENGTH];
    provided_public.copy_from_slice(&bytes[public_start..public_end]);

    let signing = backend::SigningKey::from_bytes(&secret);
    if signing.verifying_key().to_bytes() != provided_public {
        return Err(KeyEncodingError::Decoding);
    }

    Ok(signing)
}
