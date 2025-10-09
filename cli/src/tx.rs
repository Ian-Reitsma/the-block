#![allow(clippy::module_name_repetitions)]

use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::signatures::ed25519::SigningKey;
use crypto_suite::transactions::{
    canonical_payload_bytes as suite_canonical_payload_bytes, TransactionSigner,
};
use foundation_lazy::sync::Lazy;
use foundation_serialization::{Deserialize, Serialize};
use rand::rngs::OsRng;
use rand::RngCore;
use std::convert::TryInto;
use std::fmt;

#[allow(dead_code)]
const CHAIN_ID: u32 = 1;

static SIGNER: Lazy<TransactionSigner> = Lazy::new(|| TransactionSigner::from_chain_id(CHAIN_ID));

/// Signature version for transactions.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum TxVersion {
    #[default]
    Ed25519Only,
    Dual,
    DilithiumOnly,
}

/// Signature bundle for transactions.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxSignature {
    #[serde(
        default = "foundation_serialization::defaults::default",
        skip_serializing_if = "foundation_serialization::skip::is_empty"
    )]
    pub ed25519: Vec<u8>,
    #[serde(
        default = "foundation_serialization::defaults::default",
        skip_serializing_if = "foundation_serialization::skip::is_empty"
    )]
    pub dilithium: Vec<u8>,
}

/// Fee lane classification for admission and scheduling.
#[derive(
    Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default,
)]
pub enum FeeLane {
    #[default]
    Consumer,
    Industrial,
}

impl FeeLane {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            FeeLane::Consumer => "consumer",
            FeeLane::Industrial => "industrial",
        }
    }
}

impl fmt::Display for FeeLane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Unsigned transaction payload in canonical form.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct RawTxPayload {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub from_: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub to: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub amount_consumer: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub amount_industrial: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub fee: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub pct_ct: u8,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub nonce: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub memo: Vec<u8>,
}

/// Signed transaction ready for submission to the mempool.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    pub payload: RawTxPayload,
    pub public_key: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub dilithium_public_key: Vec<u8>,
    pub signature: TxSignature,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub tip: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signer_pubkeys: Vec<Vec<u8>>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub aggregate_signature: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub threshold: u8,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub lane: FeeLane,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub version: TxVersion,
}

impl Default for SignedTransaction {
    fn default() -> Self {
        Self {
            payload: RawTxPayload::default(),
            public_key: Vec::new(),
            dilithium_public_key: Vec::new(),
            signature: TxSignature::default(),
            tip: 0,
            signer_pubkeys: Vec::new(),
            aggregate_signature: Vec::new(),
            threshold: 0,
            lane: FeeLane::Consumer,
            version: TxVersion::Ed25519Only,
        }
    }
}

/// Remote attestation accompanying a DID anchor transaction.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchorAttestation {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signer: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signature: String,
}

/// Transaction anchoring a DID document on-chain.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchor {
    #[serde(default = "foundation_serialization::defaults::default")]
    pub address: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub public_key: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub document: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub nonce: u64,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signature: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub remote_attestation: Option<TxDidAnchorAttestation>,
}

impl TxDidAnchor {
    pub fn document_hash(&self) -> [u8; 32] {
        blake3::hash(self.document.as_bytes()).into()
    }

    pub fn owner_digest(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(b"did-anchor:");
        hasher.update(self.address.as_bytes());
        hasher.update(&self.document_hash());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }

    pub fn remote_digest(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(b"did-anchor-remote:");
        hasher.update(self.address.as_bytes());
        hasher.update(&self.document_hash());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }
}

/// Generate a new Ed25519 keypair (private, public).
#[allow(dead_code)]
pub fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = OsRng::default();
    let mut priv_bytes = [0u8; 32];
    rng.fill_bytes(&mut priv_bytes);
    let sk = SigningKey::from_bytes(&priv_bytes);
    let vk = sk.verifying_key();
    (priv_bytes.to_vec(), vk.to_bytes().to_vec())
}

/// Sign a transaction payload with the provided Ed25519 private key.
#[allow(dead_code)]
pub fn sign_tx(sk_bytes: &[u8], payload: &RawTxPayload) -> Option<SignedTransaction> {
    let sk_bytes = to_array_32(sk_bytes)?;
    let payload_bytes = canonical_payload_bytes(payload);
    let (sig, public_key) = SIGNER.sign_with_secret(&sk_bytes, &payload_bytes);
    let signature = TxSignature {
        ed25519: sig.to_bytes().to_vec(),
        dilithium: Vec::new(),
    };
    Some(SignedTransaction {
        payload: payload.clone(),
        public_key: public_key.to_vec(),
        dilithium_public_key: Vec::new(),
        signature,
        tip: 0,
        signer_pubkeys: Vec::new(),
        aggregate_signature: Vec::new(),
        threshold: 0,
        lane: FeeLane::Consumer,
        version: TxVersion::Ed25519Only,
    })
}

#[allow(dead_code)]
fn to_array_32(bytes: &[u8]) -> Option<[u8; 32]> {
    bytes.try_into().ok()
}

#[allow(dead_code)]
fn canonical_payload_bytes(payload: &RawTxPayload) -> Vec<u8> {
    suite_canonical_payload_bytes(payload)
}
