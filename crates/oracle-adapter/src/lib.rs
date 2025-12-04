#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher as Blake3;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::{
    Signature as Ed25519Signature, VerifyingKey, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH,
};
use energy_market::{OracleAddress, ProviderId, UnixTimestamp};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

pub type Signature = Vec<u8>;

pub trait SignatureVerifier: Send + Sync + 'static {
    fn verify(&self, provider_id: &ProviderId, payload: &[u8], signature: &[u8]) -> bool;
}

#[derive(Debug, Error)]
pub enum VerifierConfigError {
    #[error("failed to decode hex key for provider {provider_id}")]
    InvalidHex { provider_id: ProviderId },
    #[error(
        "invalid ed25519 key length for provider {provider_id}: expected {expected} bytes, got {actual}"
    )]
    InvalidLength {
        provider_id: ProviderId,
        expected: usize,
        actual: usize,
    },
    #[error("invalid ed25519 public key for provider {provider_id}: {reason}")]
    InvalidKey {
        provider_id: ProviderId,
        reason: String,
    },
}

#[derive(Clone, Default)]
pub struct Ed25519SignatureVerifier {
    inner: Arc<VerifierInner>,
}

#[derive(Default)]
struct VerifierInner {
    keys: RwLock<HashMap<ProviderId, VerifyingKey>>,
}

impl Ed25519SignatureVerifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_key_hex(
        &self,
        provider_id: ProviderId,
        hex_key: &str,
    ) -> Result<(), VerifierConfigError> {
        let bytes = hex::decode(hex_key).map_err(|_| VerifierConfigError::InvalidHex {
            provider_id: provider_id.clone(),
        })?;
        self.register_key_bytes(provider_id, &bytes)
    }

    pub fn register_key_bytes(
        &self,
        provider_id: ProviderId,
        key_bytes: &[u8],
    ) -> Result<(), VerifierConfigError> {
        if key_bytes.len() != PUBLIC_KEY_LENGTH {
            return Err(VerifierConfigError::InvalidLength {
                provider_id,
                expected: PUBLIC_KEY_LENGTH,
                actual: key_bytes.len(),
            });
        }
        let mut array = [0u8; PUBLIC_KEY_LENGTH];
        array.copy_from_slice(key_bytes);
        let verifying_key =
            VerifyingKey::from_bytes(&array).map_err(|err| VerifierConfigError::InvalidKey {
                provider_id: provider_id.clone(),
                reason: err.to_string(),
            })?;
        self.inner
            .keys
            .write()
            .expect("verifier lock poisoned")
            .insert(provider_id, verifying_key);
        Ok(())
    }

    pub fn unregister(&self, provider_id: &str) -> bool {
        self.inner
            .keys
            .write()
            .expect("verifier lock poisoned")
            .remove(provider_id)
            .is_some()
    }
}

impl SignatureVerifier for Ed25519SignatureVerifier {
    fn verify(&self, provider_id: &ProviderId, payload: &[u8], signature: &[u8]) -> bool {
        let keys = self.inner.keys.read().expect("verifier lock poisoned");
        let Some(key) = keys.get(provider_id) else {
            // Shadow mode: if no key registered, skip verification.
            return true;
        };
        if signature.len() != SIGNATURE_LENGTH {
            return false;
        }
        let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(signature);
        let sig = Ed25519Signature::from_bytes(&sig_bytes);
        key.verify(payload, &sig).is_ok()
    }
}

pub trait MeterReading {
    fn timestamp(&self) -> UnixTimestamp;
    fn provider_id(&self) -> &ProviderId;
    fn meter_address(&self) -> &OracleAddress;
    fn kwh_reading(&self) -> u64;
    fn signature(&self) -> &[u8];
    fn signing_bytes(&self) -> Vec<u8>;

    fn verify<V: SignatureVerifier>(&self, verifier: &V) -> bool {
        verifier.verify(self.provider_id(), &self.signing_bytes(), self.signature())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MeterReadingPayload {
    pub provider_id: ProviderId,
    pub meter_address: OracleAddress,
    pub kwh_reading: u64,
    pub timestamp: UnixTimestamp,
    pub signature: Signature,
}

impl MeterReadingPayload {
    pub fn new(
        provider_id: ProviderId,
        meter_address: OracleAddress,
        kwh_reading: u64,
        timestamp: UnixTimestamp,
        signature: Signature,
    ) -> Self {
        Self {
            provider_id,
            meter_address,
            kwh_reading,
            timestamp,
            signature,
        }
    }
}

impl MeterReading for MeterReadingPayload {
    fn timestamp(&self) -> UnixTimestamp {
        self.timestamp
    }

    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn meter_address(&self) -> &OracleAddress {
        &self.meter_address
    }

    fn kwh_reading(&self) -> u64 {
        self.kwh_reading
    }

    fn signature(&self) -> &[u8] {
        &self.signature
    }

    fn signing_bytes(&self) -> Vec<u8> {
        let mut hasher = Blake3::new();
        hasher.update(self.provider_id.as_bytes());
        hasher.update(self.meter_address.as_bytes());
        hasher.update(&self.kwh_reading.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}

#[derive(Debug, Error)]
pub enum OracleError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("invalid signature for provider {0}")]
    InvalidSignature(ProviderId),
    #[error("submit error: {0}")]
    Submit(String),
}

pub struct OracleAdapter<F, S, V>
where
    F: Fn(&str) -> Result<MeterReadingPayload, OracleError> + Send + Sync + 'static,
    S: Fn(&MeterReadingPayload) -> Result<(), OracleError> + Send + Sync + 'static,
    V: SignatureVerifier,
{
    fetcher: Arc<F>,
    submitter: Arc<S>,
    verifier: Arc<V>,
}

impl<F, S, V> OracleAdapter<F, S, V>
where
    F: Fn(&str) -> Result<MeterReadingPayload, OracleError> + Send + Sync + 'static,
    S: Fn(&MeterReadingPayload) -> Result<(), OracleError> + Send + Sync + 'static,
    V: SignatureVerifier,
{
    pub fn new(fetcher: F, submitter: S, verifier: V) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            submitter: Arc::new(submitter),
            verifier: Arc::new(verifier),
        }
    }

    pub async fn fetch_meter_reading(
        &self,
        meter_address: &str,
    ) -> Result<MeterReadingPayload, OracleError> {
        let reading = (self.fetcher)(meter_address)?;
        if !reading.verify(self.verifier.as_ref()) {
            return Err(OracleError::InvalidSignature(reading.provider_id.clone()));
        }
        Ok(reading)
    }

    pub fn submit_reading_to_chain(&self, reading: MeterReadingPayload) -> Result<(), OracleError> {
        (self.submitter)(&reading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::ed25519::{Signature, SigningKey, SECRET_KEY_LENGTH};

    fn fixed_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; SECRET_KEY_LENGTH])
    }

    fn sample_payload() -> MeterReadingPayload {
        MeterReadingPayload::new(
            "energy-0x01".into(),
            "meter-1".into(),
            1_250,
            1_000_000,
            Vec::new(),
        )
    }

    fn sign_payload(payload: &mut MeterReadingPayload, key: &SigningKey) {
        let message = payload.signing_bytes();
        let signature: Signature = key.sign(message.as_slice());
        payload.signature = signature.to_bytes().to_vec();
    }

    #[test]
    fn signing_bytes_hashes_payload() {
        let payload = sample_payload();
        let mut hasher = Blake3::new();
        hasher.update(payload.provider_id.as_bytes());
        hasher.update(payload.meter_address.as_bytes());
        hasher.update(&payload.kwh_reading.to_le_bytes());
        hasher.update(&payload.timestamp.to_le_bytes());
        let expected = hasher.finalize();
        assert_eq!(payload.signing_bytes(), expected.as_bytes());
    }

    #[test]
    fn verifier_accepts_valid_signature() {
        let verifier = Ed25519SignatureVerifier::new();
        let signing_key = fixed_signing_key();
        let verifying_key = signing_key.verifying_key();
        verifier
            .register_key_bytes("energy-0x01".into(), &verifying_key.to_bytes())
            .expect("register key");

        let mut payload = sample_payload();
        sign_payload(&mut payload, &signing_key);
        assert!(payload.verify(&verifier));
    }

    #[test]
    fn verifier_rejects_invalid_signature() {
        let verifier = Ed25519SignatureVerifier::new();
        let signing_key = fixed_signing_key();
        let verifying_key = signing_key.verifying_key();
        verifier
            .register_key_bytes("energy-0x01".into(), &verifying_key.to_bytes())
            .expect("register key");

        let mut payload = sample_payload();
        payload.signature = vec![0u8; SIGNATURE_LENGTH];
        assert!(!payload.verify(&verifier));
    }

    #[test]
    fn verifier_skips_unregistered_provider() {
        let verifier = Ed25519SignatureVerifier::new();
        let mut payload = sample_payload();
        payload.signature = vec![0u8; SIGNATURE_LENGTH];
        assert!(payload.verify(&verifier));
    }
}
