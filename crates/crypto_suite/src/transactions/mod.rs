use core::ops::Deref;

use foundation_serialization::Serialize;
use thiserror::Error;

use codec::{profiles, BinaryConfig};

use crate::signatures::ed25519::{
    Signature, SignatureError, SigningKey, VerifyingKey, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH,
};

/// Domain prefix shared by all transaction-signing contexts.
pub const TRANSACTION_DOMAIN_PREFIX: &[u8; 12] = b"THE_BLOCKv2|";

/// Error type covering canonical serialization and signature checks.
#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("serialization failed: {0}")]
    Serialization(#[from] codec::Error),
    #[error("signature verification failed: {0}")]
    Signature(#[from] SignatureError),
}

/// Lightweight newtype around the 16-byte domain tag used for transaction signing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DomainTag([u8; 16]);

impl DomainTag {
    /// Construct a domain tag from raw bytes.
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Borrow the inner bytes.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Consume the tag and return the underlying byte array.
    pub const fn into_inner(self) -> [u8; 16] {
        self.0
    }
}

impl From<DomainTag> for [u8; 16] {
    fn from(value: DomainTag) -> Self {
        value.into_inner()
    }
}

impl Deref for DomainTag {
    type Target = [u8; 16];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

/// Compute the canonical 16-byte domain separation tag for the provided chain identifier.
#[must_use]
pub fn domain_tag_for(chain_id: u32) -> DomainTag {
    let mut buf = [0u8; 16];
    buf[..TRANSACTION_DOMAIN_PREFIX.len()].copy_from_slice(TRANSACTION_DOMAIN_PREFIX);
    buf[TRANSACTION_DOMAIN_PREFIX.len()..TRANSACTION_DOMAIN_PREFIX.len() + 4]
        .copy_from_slice(&chain_id.to_le_bytes());
    DomainTag(buf)
}

/// Canonical binary configuration shared across transaction serialization helpers.
#[must_use]
pub fn canonical_binary_config() -> BinaryConfig {
    profiles::transaction::config()
}

/// Serialize a payload using the canonical transaction binary settings.
#[must_use]
pub fn canonical_payload_bytes<T>(payload: &T) -> Vec<u8>
where
    T: Serialize,
{
    try_canonical_payload_bytes(payload)
        .unwrap_or_else(|err| panic!("failed to serialize payload: {err}"))
}

/// Serialize a payload using canonical settings, returning the error instead of panicking.
pub fn try_canonical_payload_bytes<T>(payload: &T) -> Result<Vec<u8>, codec::Error>
where
    T: Serialize,
{
    codec::serialize(profiles::transaction::codec(), payload)
}

/// Helper that attaches the configured domain tag to a message payload.
#[must_use]
pub fn domain_separated_message(domain: &DomainTag, payload: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(domain.as_bytes().len() + payload.len());
    msg.extend_from_slice(domain.as_bytes());
    msg.extend_from_slice(payload);
    msg
}

/// Stateless helper for signing and verifying transactions with an explicit domain tag.
#[derive(Clone, Debug)]
pub struct TransactionSigner {
    domain: DomainTag,
}

impl TransactionSigner {
    /// Construct a signer for the provided chain identifier.
    #[must_use]
    pub fn from_chain_id(chain_id: u32) -> Self {
        Self {
            domain: domain_tag_for(chain_id),
        }
    }

    /// Construct a signer from a precomputed domain tag.
    #[must_use]
    pub const fn new(domain: DomainTag) -> Self {
        Self { domain }
    }

    /// Borrow the domain tag.
    #[must_use]
    pub const fn domain(&self) -> &DomainTag {
        &self.domain
    }

    /// Produce the domain-separated message for the provided payload bytes.
    #[must_use]
    pub fn message(&self, payload: &[u8]) -> Vec<u8> {
        domain_separated_message(&self.domain, payload)
    }

    /// Sign the payload bytes with the provided signing key.
    #[must_use]
    pub fn sign(&self, key: &SigningKey, payload: &[u8]) -> Signature {
        let msg = self.message(payload);
        key.sign(&msg)
    }

    /// Sign payload bytes using raw secret-key material and return the detached signature
    /// alongside the verifying key bytes.
    #[must_use]
    pub fn sign_with_secret(
        &self,
        secret: &[u8; SECRET_KEY_LENGTH],
        payload: &[u8],
    ) -> (Signature, [u8; PUBLIC_KEY_LENGTH]) {
        let key = SigningKey::from_bytes(secret);
        let signature = self.sign(&key, payload);
        (signature, key.verifying_key().to_bytes())
    }

    /// Attempt to verify a signature against the supplied verifying key and payload bytes.
    pub fn verify(
        &self,
        verifying_key: &VerifyingKey,
        payload: &[u8],
        signature: &Signature,
    ) -> Result<(), SignatureError> {
        let msg = self.message(payload);
        verifying_key.verify(&msg, signature)
    }

    /// Verify a signature against raw verifying-key bytes.
    pub fn verify_with_public_bytes(
        &self,
        verifying_key: &[u8; PUBLIC_KEY_LENGTH],
        payload: &[u8],
        signature: &Signature,
    ) -> Result<(), SignatureError> {
        let vk = VerifyingKey::from_bytes(verifying_key)?;
        self.verify(&vk, payload, signature)
    }

    /// Sign a serializable payload using the canonical encoder and return the signature together
    /// with the verifying key bytes.
    pub fn sign_serialized<T>(
        &self,
        secret: &[u8; SECRET_KEY_LENGTH],
        payload: &T,
    ) -> Result<(Signature, [u8; PUBLIC_KEY_LENGTH]), TransactionError>
    where
        T: Serialize,
    {
        let bytes = try_canonical_payload_bytes(payload)?;
        Ok(self.sign_with_secret(secret, &bytes))
    }

    /// Verify a serialized payload against the provided verifying-key bytes and detached signature.
    pub fn verify_serialized<T>(
        &self,
        verifying_key: &[u8; PUBLIC_KEY_LENGTH],
        payload: &T,
        signature: &Signature,
    ) -> Result<(), TransactionError>
    where
        T: Serialize,
    {
        let bytes = try_canonical_payload_bytes(payload)?;
        self.verify_with_public_bytes(verifying_key, &bytes, signature)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    use super::*;

    #[derive(foundation_serialization::Serialize)]
    struct Payload<'a> {
        name: &'a str,
        value: u64,
    }

    #[test]
    fn canonical_serialization_matches_config_round_trip() {
        let payload = Payload {
            name: "alice",
            value: 42,
        };
        let via_helper = try_canonical_payload_bytes(&payload).expect("helper serialization");
        let via_config = canonical_binary_config()
            .serialize(&payload)
            .expect("config serialization");
        assert_eq!(via_helper, via_config);
    }

    #[test]
    fn signing_and_verification_round_trip() {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&7u64.to_le_bytes());
        let mut rng = <StdRng as SeedableRng>::from_seed(seed);
        let signing_key = SigningKey::generate(&mut rng);
        let signer = TransactionSigner::from_chain_id(1);
        let payload = b"payload";

        let signature = signer.sign(&signing_key, payload);
        let verifying_key = signing_key.verifying_key();
        assert!(signer.verify(&verifying_key, payload, &signature).is_ok());

        let wrong_signer = TransactionSigner::from_chain_id(2);
        assert!(wrong_signer
            .verify(&verifying_key, payload, &signature)
            .is_err());
    }
}
