use crypto_suite::signatures::ed25519::Signature;
use foundation_serialization::{Deserialize, Serialize};

/// A minimal partially signed block transaction container used for
/// air-gapped signing workflows. The payload is an opaque blob (typically a
/// serialized block header or transaction) accompanied by a set of
/// hex-encoded signatures.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Psbt {
    /// Opaque payload to be signed.
    pub payload: Vec<u8>,
    /// Collected signatures in lowercase hex.
    pub signatures: Vec<String>,
}

impl Psbt {
    /// Create a new container from a payload.
    pub fn new(payload: Vec<u8>) -> Self {
        Self {
            payload,
            signatures: Vec::new(),
        }
    }

    /// Append an Ed25519 signature to the container.
    pub fn add_signature(&mut self, sig: Signature) {
        self.signatures
            .push(crypto_suite::hex::encode(sig.to_bytes()));
    }
}
