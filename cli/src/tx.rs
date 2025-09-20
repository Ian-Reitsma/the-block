#![allow(clippy::module_name_repetitions)]

use bincode::Options;
use blake3::Hasher;
use ed25519_dalek::{Signer, SigningKey};
use once_cell::sync::Lazy;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

#[allow(dead_code)]
const CHAIN_ID: u32 = 1;
#[allow(dead_code)]
const DOMAIN_PREFIX: &[u8; 12] = b"THE_BLOCKv2|";

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ed25519: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dilithium: Vec<u8>,
}

/// Fee lane classification for admission and scheduling.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
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

/// Unsigned transaction payload in canonical form.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct RawTxPayload {
    #[serde(default)]
    pub from_: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub amount_consumer: u64,
    #[serde(default)]
    pub amount_industrial: u64,
    #[serde(default)]
    pub fee: u64,
    #[serde(default)]
    pub pct_ct: u8,
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub memo: Vec<u8>,
}

/// Signed transaction ready for submission to the mempool.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    pub payload: RawTxPayload,
    pub public_key: Vec<u8>,
    #[serde(default)]
    pub dilithium_public_key: Vec<u8>,
    pub signature: TxSignature,
    #[serde(default)]
    pub tip: u64,
    #[serde(default)]
    pub signer_pubkeys: Vec<Vec<u8>>,
    #[serde(default)]
    pub aggregate_signature: Vec<u8>,
    #[serde(default)]
    pub threshold: u8,
    #[serde(default)]
    pub lane: FeeLane,
    #[serde(default)]
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
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub signature: String,
}

/// Transaction anchoring a DID document on-chain.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchor {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub public_key: Vec<u8>,
    #[serde(default)]
    pub document: String,
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub signature: Vec<u8>,
    #[serde(default)]
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
    let mut rng = OsRng;
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
    let sk = SigningKey::from_bytes(&sk_bytes);
    let msg = {
        let mut m = domain_tag().to_vec();
        m.extend(canonical_payload_bytes(payload));
        m
    };
    let sig = sk.sign(&msg);
    let signature = TxSignature {
        ed25519: sig.to_bytes().to_vec(),
        dilithium: Vec::new(),
    };
    Some(SignedTransaction {
        payload: payload.clone(),
        public_key: sk.verifying_key().to_bytes().to_vec(),
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
    bincode_config()
        .serialize(payload)
        .unwrap_or_else(|e| panic!("serialize: {e}"))
}

#[allow(dead_code)]
fn domain_tag() -> &'static [u8] {
    static TAG: Lazy<[u8; 16]> = Lazy::new(|| domain_tag_for(CHAIN_ID));
    &*TAG
}

#[allow(dead_code)]
fn domain_tag_for(id: u32) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..DOMAIN_PREFIX.len()].copy_from_slice(DOMAIN_PREFIX);
    buf[DOMAIN_PREFIX.len()..DOMAIN_PREFIX.len() + 4].copy_from_slice(&id.to_le_bytes());
    buf
}

#[allow(dead_code)]
fn bincode_config() -> bincode::config::WithOtherEndian<
    bincode::config::WithOtherIntEncoding<bincode::DefaultOptions, bincode::config::FixintEncoding>,
    bincode::config::LittleEndian,
> {
    static CFG: Lazy<
        bincode::config::WithOtherEndian<
            bincode::config::WithOtherIntEncoding<
                bincode::DefaultOptions,
                bincode::config::FixintEncoding,
            >,
            bincode::config::LittleEndian,
        >,
    > = Lazy::new(|| {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
    });
    *CFG
}
