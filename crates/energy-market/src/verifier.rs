#![forbid(unsafe_code)]

//! Oracle signature verification for meter readings.
//!
//! Supports multiple signature schemes via the `SignatureVerifier` trait:
//! - Ed25519 (always available)
//! - Dilithium (behind `pq-crypto` feature flag)
//!
//! The verifier registry allows governance to configure which schemes
//! are accepted for each provider.

use crate::{MeterReading, ProviderId};
use crypto_suite::hashing::blake3::Hasher as Blake3;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("signature scheme {0} not supported")]
    UnsupportedScheme(String),

    #[error("invalid signature for provider {provider_id}: {reason}")]
    InvalidSignature {
        provider_id: ProviderId,
        reason: String,
    },

    #[error("provider {0} not registered in verifier registry")]
    ProviderNotRegistered(ProviderId),

    #[error("malformed signature bytes: {0}")]
    MalformedSignature(String),

    #[error("malformed public key: {0}")]
    MalformedPublicKey(String),
}

impl VerificationError {
    pub fn label(&self) -> &'static str {
        match self {
            VerificationError::UnsupportedScheme(_) => "unsupported_scheme",
            VerificationError::InvalidSignature { .. } => "invalid_signature",
            VerificationError::ProviderNotRegistered(_) => "provider_not_registered",
            VerificationError::MalformedSignature(_) => "malformed_signature",
            VerificationError::MalformedPublicKey(_) => "malformed_public_key",
        }
    }
}

/// Signature scheme identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum SignatureScheme {
    Ed25519,
    #[cfg(feature = "pq-crypto")]
    Dilithium3,
    #[cfg(feature = "pq-crypto")]
    Dilithium5,
}

impl SignatureScheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            #[cfg(feature = "pq-crypto")]
            Self::Dilithium3 => "dilithium3",
            #[cfg(feature = "pq-crypto")]
            Self::Dilithium5 => "dilithium5",
        }
    }

    pub fn parse(s: &str) -> Result<Self, VerificationError> {
        match s.to_lowercase().as_str() {
            "ed25519" => Ok(Self::Ed25519),
            #[cfg(feature = "pq-crypto")]
            "dilithium3" => Ok(Self::Dilithium3),
            #[cfg(feature = "pq-crypto")]
            "dilithium5" => Ok(Self::Dilithium5),
            _ => Err(VerificationError::UnsupportedScheme(s.to_string())),
        }
    }
}

impl FromStr for SignatureScheme {
    type Err = VerificationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SignatureScheme::parse(s)
    }
}

/// Trait for signature verification providers
pub trait SignatureVerifier: Send + Sync {
    /// Verify a signature over the canonical meter reading payload
    fn verify(&self, reading: &MeterReading, public_key: &[u8]) -> Result<(), VerificationError>;

    /// Return the scheme identifier
    fn scheme(&self) -> SignatureScheme;
}

/// Ed25519 signature verifier (always available)
pub struct Ed25519Verifier;

impl SignatureVerifier for Ed25519Verifier {
    fn verify(&self, reading: &MeterReading, public_key: &[u8]) -> Result<(), VerificationError> {
        // Extract 32-byte public key
        if public_key.len() != 32 {
            return Err(VerificationError::MalformedPublicKey(format!(
                "expected 32 bytes, got {}",
                public_key.len()
            )));
        }

        // Extract 64-byte signature
        if reading.signature.len() != 64 {
            return Err(VerificationError::MalformedSignature(format!(
                "expected 64 bytes, got {}",
                reading.signature.len()
            )));
        }

        // Compute canonical message to sign: BLAKE3(provider_id || meter_address || total_kwh || timestamp)
        let mut hasher = Blake3::new();
        hasher.update(reading.provider_id.as_bytes());
        hasher.update(reading.meter_address.as_bytes());
        hasher.update(&reading.total_kwh.to_le_bytes());
        hasher.update(&reading.timestamp.to_le_bytes());
        let message = hasher.finalize();

        // Verify signature using crypto_suite
        let pk_array: &[u8; 32] = public_key
            .try_into()
            .map_err(|_| VerificationError::MalformedPublicKey("conversion failed".into()))?;

        let vk =
            crypto_suite::signatures::ed25519::VerifyingKey::from_bytes(pk_array).map_err(|e| {
                VerificationError::MalformedPublicKey(format!("ed25519 key parse failed: {}", e))
            })?;

        let sig_array: &[u8; 64] = reading.signature[..]
            .try_into()
            .map_err(|_| VerificationError::MalformedSignature("conversion failed".into()))?;

        let sig = crypto_suite::signatures::ed25519::Signature::from_bytes(sig_array);

        vk.verify(message.as_bytes(), &sig)
            .map_err(|e| VerificationError::InvalidSignature {
                provider_id: reading.provider_id.clone(),
                reason: format!("ed25519 verification failed: {}", e),
            })
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::Ed25519
    }
}

#[cfg(feature = "pq-crypto")]
pub struct DilithiumVerifier {
    level: u8,
}

#[cfg(feature = "pq-crypto")]
impl DilithiumVerifier {
    pub fn new_level3() -> Self {
        Self { level: 3 }
    }

    pub fn new_level5() -> Self {
        Self { level: 5 }
    }
}

#[cfg(feature = "pq-crypto")]
impl SignatureVerifier for DilithiumVerifier {
    fn verify(&self, reading: &MeterReading, public_key: &[u8]) -> Result<(), VerificationError> {
        // Compute canonical message
        let mut hasher = Blake3::new();
        hasher.update(reading.provider_id.as_bytes());
        hasher.update(reading.meter_address.as_bytes());
        hasher.update(&reading.total_kwh.to_le_bytes());
        hasher.update(&reading.timestamp.to_le_bytes());
        let message = hasher.finalize();

        // Verify using pqcrypto-dilithium
        match self.level {
            3 => {
                use pqcrypto_dilithium::dilithium3::*;
                let pk = PublicKey::from_bytes(public_key).map_err(|e| {
                    VerificationError::MalformedPublicKey(format!("dilithium3: {}", e))
                })?;
                let sig = DetachedSignature::from_bytes(&reading.signature).map_err(|e| {
                    VerificationError::MalformedSignature(format!("dilithium3: {}", e))
                })?;
                verify_detached_signature(&sig, message.as_bytes(), &pk).map_err(|e| {
                    VerificationError::InvalidSignature {
                        provider_id: reading.provider_id.clone(),
                        reason: format!("dilithium3 verification failed: {}", e),
                    }
                })
            }
            5 => {
                // TEMPORARY: dilithium5 not yet available in pqcrypto_dilithium, using dilithium3 as fallback
                use pqcrypto_dilithium::dilithium3::*;
                let pk = PublicKey::from_bytes(public_key).map_err(|e| {
                    VerificationError::MalformedPublicKey(format!(
                        "dilithium3 (level5 fallback): {}",
                        e
                    ))
                })?;
                let sig = DetachedSignature::from_bytes(&reading.signature).map_err(|e| {
                    VerificationError::MalformedSignature(format!(
                        "dilithium3 (level5 fallback): {}",
                        e
                    ))
                })?;
                verify_detached_signature(&sig, message.as_bytes(), &pk).map_err(|e| {
                    VerificationError::InvalidSignature {
                        provider_id: reading.provider_id.clone(),
                        reason: format!("dilithium3 (level5 fallback) verification failed: {}", e),
                    }
                })
            }
            _ => Err(VerificationError::UnsupportedScheme(format!(
                "dilithium level {}",
                self.level
            ))),
        }
    }

    fn scheme(&self) -> SignatureScheme {
        match self.level {
            3 => SignatureScheme::Dilithium3,
            5 => SignatureScheme::Dilithium5,
            _ => SignatureScheme::Dilithium3, // fallback
        }
    }
}

/// Provider public key registration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderKey {
    pub provider_id: ProviderId,
    pub public_key: Vec<u8>,
    pub scheme: SignatureScheme,
}

/// Registry of provider keys and allowed signature schemes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct VerifierRegistry {
    provider_keys: BTreeMap<ProviderId, ProviderKey>,
}

impl VerifierRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.provider_keys.clear();
    }

    pub fn replace_all<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = ProviderKey>,
    {
        self.provider_keys.clear();
        for entry in entries {
            self.provider_keys.insert(entry.provider_id.clone(), entry);
        }
    }

    /// Register a provider's public key
    pub fn register(
        &mut self,
        provider_id: ProviderId,
        public_key: Vec<u8>,
        scheme: SignatureScheme,
    ) {
        self.provider_keys.insert(
            provider_id.clone(),
            ProviderKey {
                provider_id,
                public_key,
                scheme,
            },
        );
    }

    /// Unregister a provider's key
    pub fn unregister(&mut self, provider_id: &str) -> Option<ProviderKey> {
        self.provider_keys.remove(provider_id)
    }

    /// Get a provider's registered key
    pub fn get(&self, provider_id: &str) -> Option<&ProviderKey> {
        self.provider_keys.get(provider_id)
    }

    /// Verify a meter reading signature
    pub fn verify(&self, reading: &MeterReading) -> Result<(), VerificationError> {
        let key = self
            .get(&reading.provider_id)
            .ok_or_else(|| VerificationError::ProviderNotRegistered(reading.provider_id.clone()))?;

        let verifier: Box<dyn SignatureVerifier> = match key.scheme {
            SignatureScheme::Ed25519 => Box::new(Ed25519Verifier),
            #[cfg(feature = "pq-crypto")]
            SignatureScheme::Dilithium3 => Box::new(DilithiumVerifier::new_level3()),
            #[cfg(feature = "pq-crypto")]
            SignatureScheme::Dilithium5 => Box::new(DilithiumVerifier::new_level5()),
        };

        verifier.verify(reading, &key.public_key)
    }

    /// List all registered providers
    pub fn providers(&self) -> Vec<&str> {
        self.provider_keys.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_roundtrip() {
        assert_eq!(
            SignatureScheme::parse("ed25519").unwrap(),
            SignatureScheme::Ed25519
        );
        assert_eq!(SignatureScheme::Ed25519.as_str(), "ed25519");
    }

    #[test]
    fn registry_register_unregister() {
        let mut registry = VerifierRegistry::new();
        registry.register(
            "provider-1".to_string(),
            vec![0u8; 32],
            SignatureScheme::Ed25519,
        );

        assert!(registry.get("provider-1").is_some());
        assert!(registry.get("provider-2").is_none());

        let removed = registry.unregister("provider-1");
        assert!(removed.is_some());
        assert!(registry.get("provider-1").is_none());
    }

    #[test]
    fn registry_replace_all_swaps_entries() {
        let mut registry = VerifierRegistry::new();
        registry.register(
            "provider-1".to_string(),
            vec![0u8; 32],
            SignatureScheme::Ed25519,
        );
        assert_eq!(registry.providers().len(), 1);
        registry.replace_all(vec![ProviderKey {
            provider_id: "provider-2".into(),
            public_key: vec![1u8; 32],
            scheme: SignatureScheme::Ed25519,
        }]);
        assert!(registry.get("provider-1").is_none());
        assert!(registry.get("provider-2").is_some());
    }
}
