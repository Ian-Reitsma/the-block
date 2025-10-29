use core::fmt;

use crate::hashing::blake3;
use crate::signatures::ed25519;

/// Domain separation string applied to all VRF transcripts produced by the
/// in-house implementation.
const TRANSCRIPT_LABEL: &[u8] = b"the-block.vrf.transcript";

/// Length in bytes of the VRF output digest.
pub const OUTPUT_LENGTH: usize = 32;

/// Length in bytes of the serialized VRF proof (Ed25519 signature).
pub const PROOF_LENGTH: usize = ed25519::SIGNATURE_LENGTH;

/// Length in bytes of a serialized public key.
pub const PUBLIC_KEY_LENGTH: usize = ed25519::PUBLIC_KEY_LENGTH;

/// Length in bytes of a serialized secret key.
pub const SECRET_KEY_LENGTH: usize = ed25519::SECRET_KEY_LENGTH;

/// Errors produced when verifying a VRF proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VrfError {
    /// The provided proof failed signature verification against the public key.
    InvalidProof,
    /// The serialized proof was not the expected length.
    InvalidLength,
    /// The serialized public key was malformed.
    InvalidKey,
}

impl fmt::Display for VrfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrfError::InvalidProof => write!(f, "invalid vrf proof"),
            VrfError::InvalidLength => write!(f, "invalid vrf proof length"),
            VrfError::InvalidKey => write!(f, "invalid vrf public key"),
        }
    }
}

impl std::error::Error for VrfError {}

/// Secret key capable of producing VRF proofs.
#[derive(Clone)]
pub struct SecretKey(ed25519::SigningKey);

impl SecretKey {
    /// Construct a VRF secret key from an Ed25519 signing key.
    pub fn from_signing_key(key: ed25519::SigningKey) -> Self {
        Self(key)
    }

    /// Generate a fresh VRF keypair using the supplied RNG.
    pub fn generate<R>(rng: &mut R) -> (Self, PublicKey)
    where
        R: rand::CryptoRng + rand::RngCore,
    {
        let signing = ed25519::SigningKey::generate(rng);
        let verifying = signing.verifying_key();
        (Self(signing), PublicKey(verifying))
    }

    /// Serialize the secret key to raw bytes.
    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.0.to_bytes()
    }

    /// Deserialize the secret key from raw bytes.
    pub fn from_bytes(bytes: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self(ed25519::SigningKey::from_bytes(bytes))
    }

    /// Produce a deterministic VRF output and proof for the supplied transcript.
    pub fn evaluate(&self, context: &[u8], transcript: &[u8]) -> (Output, Proof) {
        let challenge = build_challenge(context, transcript);
        let signature = self.0.sign(challenge.as_bytes());
        let output = derive_output(signature.to_bytes(), challenge.as_bytes());
        (Output(output), Proof(signature.to_bytes()))
    }

    /// Return the verifying key associated with this secret key.
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }
}

/// Public key required to verify VRF proofs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey(ed25519::VerifyingKey);

impl PublicKey {
    /// Construct from an Ed25519 verifying key.
    pub fn from_verifying_key(key: ed25519::VerifyingKey) -> Self {
        Self(key)
    }

    /// Deserialize from raw bytes.
    pub fn from_bytes(bytes: &[u8; PUBLIC_KEY_LENGTH]) -> Result<Self, VrfError> {
        ed25519::VerifyingKey::from_bytes(bytes)
            .map(Self)
            .map_err(|_| VrfError::InvalidKey)
    }

    /// Serialize to raw bytes.
    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.0.to_bytes()
    }

    /// Verify a VRF proof against the provided transcript, returning the VRF output.
    pub fn verify(
        &self,
        context: &[u8],
        transcript: &[u8],
        proof: &Proof,
    ) -> Result<Output, VrfError> {
        let challenge = build_challenge(context, transcript);
        self.0
            .verify(challenge.as_bytes(), &proof.signature())
            .map_err(|_| VrfError::InvalidProof)?;
        let output = derive_output(proof.0, challenge.as_bytes());
        Ok(Output(output))
    }
}

/// VRF proof produced alongside the output. Internally this is an Ed25519 signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proof([u8; PROOF_LENGTH]);

impl Proof {
    /// Deserialize a proof from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VrfError> {
        if bytes.len() != PROOF_LENGTH {
            return Err(VrfError::InvalidLength);
        }
        let mut buf = [0u8; PROOF_LENGTH];
        buf.copy_from_slice(bytes);
        Ok(Self(buf))
    }

    /// Serialize the proof to raw bytes.
    pub fn to_bytes(&self) -> [u8; PROOF_LENGTH] {
        self.0
    }

    fn signature(&self) -> ed25519::Signature {
        ed25519::Signature::from_bytes(&self.0)
    }
}

/// VRF output digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output([u8; OUTPUT_LENGTH]);

impl Output {
    /// Construct an output from raw bytes.
    pub fn from_bytes(bytes: [u8; OUTPUT_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Return the output as a byte slice.
    pub fn as_bytes(&self) -> &[u8; OUTPUT_LENGTH] {
        &self.0
    }

    /// Convert into the inner byte array.
    pub fn into_bytes(self) -> [u8; OUTPUT_LENGTH] {
        self.0
    }
}

fn build_challenge(context: &[u8], transcript: &[u8]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(TRANSCRIPT_LABEL);
    hasher.update(context);
    hasher.update(transcript);
    hasher.finalize()
}

fn derive_output(signature: [u8; PROOF_LENGTH], challenge: &[u8]) -> [u8; OUTPUT_LENGTH] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(signature.as_ref());
    hasher.update(challenge);
    hasher.finalize().into()
}

impl From<ed25519::VerifyingKey> for PublicKey {
    fn from(value: ed25519::VerifyingKey) -> Self {
        Self(value)
    }
}

impl From<PublicKey> for [u8; PUBLIC_KEY_LENGTH] {
    fn from(value: PublicKey) -> Self {
        value.to_bytes()
    }
}

impl From<Proof> for [u8; PROOF_LENGTH] {
    fn from(value: Proof) -> Self {
        value.to_bytes()
    }
}

impl TryFrom<&[u8]> for Proof {
    type Error = VrfError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Proof::from_bytes(value)
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", crate::hex::encode(self.0))
    }
}

#[cfg(feature = "telemetry")]
impl crate::telemetry::Recordable for PublicKey {
    fn record(&self, labels: &mut crate::telemetry::Labels) {
        labels.insert("vrf_public_key", crate::hex::encode(self.0.to_bytes()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn vrf_round_trip() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xDEADBEEF);
        let (secret, public) = SecretKey::generate(&mut rng);
        let context = b"committee-selection";
        let transcript = b"epoch-42";
        let (output, proof) = secret.evaluate(context, transcript);
        let verified = public
            .verify(context, transcript, &proof)
            .expect("proof must verify");
        assert_eq!(output, verified);
    }

    #[test]
    fn vrf_rejects_modified_transcript() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xCAFEBABE);
        let (secret, public) = SecretKey::generate(&mut rng);
        let (output, proof) = secret.evaluate(b"ctx", b"transcript");
        assert_eq!(output.as_bytes().len(), OUTPUT_LENGTH);
        assert!(matches!(
            public.verify(b"ctx", b"tampered", &proof),
            Err(VrfError::InvalidProof)
        ));
    }

    #[test]
    fn vrf_rejects_malformed_proof() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xA5A5A5A5);
        let (secret, public) = SecretKey::generate(&mut rng);
        let (_output, proof) = secret.evaluate(b"ctx", b"transcript");
        let mut tampered = proof.to_bytes();
        tampered[0] ^= 0x01;
        let bad = Proof::from_bytes(&tampered).expect("length ok");
        assert!(matches!(
            public.verify(b"ctx", b"transcript", &bad),
            Err(VrfError::InvalidProof)
        ));
    }
}
