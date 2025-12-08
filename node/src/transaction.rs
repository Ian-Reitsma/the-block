//! Transaction data structures and signing utilities.
//!
//! Exposes Python bindings for constructing, signing, and verifying
//! transactions using Ed25519 with domain separation.

pub mod binary;

#[cfg(feature = "python-bindings")]
use crate::py::{getter, setter};
use crate::py::{PyError, PyResult};
use crate::{to_array_32, to_array_64};
use concurrency::{cache::LruCache, Lazy, MutexExt};
#[cfg(feature = "quantum")]
use crypto::dilithium;
use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use crypto_suite::transactions::TransactionSigner;
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::{self, ShardId};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use self::binary::{decode_raw_payload, encode_raw_payload, encode_signed_transaction};

fn py_value_err(msg: impl Into<String>) -> PyError {
    PyError::value(msg)
}

#[allow(dead_code)]
fn py_type_err(msg: impl Into<String>) -> PyError {
    PyError::value(msg)
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum TxVersion {
    Ed25519Only,
    Dual,
    DilithiumOnly,
}

impl Default for TxVersion {
    fn default() -> Self {
        TxVersion::Ed25519Only
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct TxSignature {
    #[serde(
        default = "foundation_serialization::defaults::default",
        skip_serializing_if = "foundation_serialization::skip::is_empty"
    )]
    pub ed25519: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[serde(
        default = "foundation_serialization::defaults::default",
        skip_serializing_if = "foundation_serialization::skip::is_empty"
    )]
    pub dilithium: Vec<u8>,
}
impl Default for TxSignature {
    fn default() -> Self {
        Self {
            ed25519: Vec::new(),
            #[cfg(feature = "quantum")]
            dilithium: Vec::new(),
        }
    }
}

use crate::{fee, fee::FeeError, TxAdmissionError};

static SIG_CACHE: Lazy<Mutex<LruCache<[u8; 32], bool>>> = Lazy::new(|| {
    Mutex::new(LruCache::new(
        NonZeroUsize::new(1024).expect("cache size non-zero"),
    ))
});

static TX_SIGNER: Lazy<TransactionSigner> =
    Lazy::new(|| TransactionSigner::from_chain_id(crate::constants::CHAIN_ID));

/// Remote attestation accompanying a DID anchor transaction.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchorAttestation {
    /// Hex-encoded verifying key for the remote signer.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signer: String,
    /// Hex-encoded signature over the attestation message.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signature: String,
}

/// Specialized transaction anchoring a DID document hash on-chain.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchor {
    /// Account address owning the DID document.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub address: String,
    /// Public key authorizing the update.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub public_key: Vec<u8>,
    /// Canonical DID document body.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub document: String,
    /// Monotonic nonce protecting against replay.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub nonce: u64,
    /// Ed25519 signature from the owner over the anchor digest.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signature: Vec<u8>,
    /// Optional remote attestation signed by a provenance-configured key.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub remote_attestation: Option<TxDidAnchorAttestation>,
}

impl TxDidAnchor {
    /// Compute the BLAKE3 hash of the DID document.
    #[must_use]
    pub fn document_hash(&self) -> [u8; 32] {
        blake3::hash(self.document.as_bytes()).into()
    }

    /// Canonical digest used for owner signatures.
    #[must_use]
    pub fn owner_digest(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(b"did-anchor:");
        hasher.update(self.address.as_bytes());
        hasher.update(&self.document_hash());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }

    /// Canonical digest used for remote attestations.
    #[must_use]
    pub fn remote_digest(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(b"did-anchor-remote:");
        hasher.update(self.address.as_bytes());
        hasher.update(&self.document_hash());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.finalize().into()
    }
}

/// Distinct fee lanes for transaction scheduling.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FeeLane {
    /// Standard retail transactions sharing the consumer lane.
    Consumer,
    /// High-throughput industrial transactions.
    Industrial,
}

impl FeeLane {
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

impl Default for FeeLane {
    fn default() -> Self {
        FeeLane::Consumer
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct RawTxPayload {
    pub from_: String,
    pub to: String,
    pub amount_consumer: u64,
    pub amount_industrial: u64,
    pub fee: u64,
    pub pct_ct: u8,
    pub nonce: u64,
    pub memo: Vec<u8>,
}

impl RawTxPayload {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from_: String,
        to: String,
        amount_consumer: u64,
        amount_industrial: u64,
        fee: u64,
        pct_ct: u8,
        nonce: u64,
        memo: Vec<u8>,
    ) -> Self {
        RawTxPayload {
            from_,
            to,
            amount_consumer,
            amount_industrial,
            fee,
            pct_ct,
            nonce,
            memo,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "RawTxPayload(from='{}', to='{}', amount_consumer={}, amount_industrial={}, fee={}, pct_ct={}, nonce={}, memo_len={})",
            self.from_,
            self.to,
            self.amount_consumer,
            self.amount_industrial,
            self.fee,
            self.pct_ct,
            self.nonce,
            self.memo.len(),
        )
    }

    // Python alias property: expose `from` alongside `from_` for ergonomics
    #[cfg_attr(feature = "python-bindings", getter(from))]
    #[allow(dead_code)]
    fn get_from_alias(&self) -> String {
        self.from_.clone()
    }

    #[cfg_attr(feature = "python-bindings", setter(from))]
    #[allow(dead_code)]
    fn set_from_alias(&mut self, val: String) {
        self.from_ = val;
    }
}

/// Wrapper for transactions spanning multiple shards.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct CrossShardEnvelope<T> {
    /// Origin shard where the transaction is executed.
    pub origin: ShardId,
    /// Destination shard receiving any resulting state changes.
    pub destination: ShardId,
    /// Inner transaction payload.
    pub payload: T,
}

impl<T> CrossShardEnvelope<T> {
    /// Create a new cross-shard envelope.
    pub fn new(origin: ShardId, destination: ShardId, payload: T) -> Self {
        Self {
            origin,
            destination,
            payload,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    pub payload: RawTxPayload,
    pub public_key: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[serde(default = "foundation_serialization::defaults::default")]
    pub dilithium_public_key: Vec<u8>,
    pub signature: TxSignature,
    /// Priority fee paid to the miner above the base fee.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub tip: u64,
    /// Optional set of signer public keys for multisig.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub signer_pubkeys: Vec<Vec<u8>>,
    /// Aggregated signatures concatenated in order.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub aggregate_signature: Vec<u8>,
    /// Required number of signatures.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub threshold: u8,
    /// Fee lane classification for admission and scheduling.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub lane: FeeLane,
    /// Signature mode for the transaction.
    #[serde(default = "foundation_serialization::defaults::default")]
    pub version: TxVersion,
}
impl Default for SignedTransaction {
    fn default() -> Self {
        Self {
            payload: RawTxPayload::default(),
            public_key: Vec::new(),
            #[cfg(feature = "quantum")]
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

/// Helper methods for cross-shard envelopes carrying signed transactions.
impl CrossShardEnvelope<SignedTransaction> {
    /// Wrap a transaction with its source and destination shard identifiers.
    pub fn route(tx: SignedTransaction) -> Self {
        let from_shard = address::shard_id(&tx.payload.from_);
        let to_shard = address::shard_id(&tx.payload.to);
        Self::new(from_shard, to_shard, tx)
    }
}

/// Result of attempting to apply a transaction to state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransactionResult {
    Applied(SignedTransaction),
    Rejected {
        tx: SignedTransaction,
        error: TxAdmissionError,
    },
}

/// Perform stateless checks (signature, fee selector) before stateful execution.
pub fn verify_stateless(tx: &SignedTransaction) -> Result<(), TxAdmissionError> {
    if !verify_signed_tx(tx) {
        return Err(TxAdmissionError::BadSignature);
    }
    match fee::decompose(tx.payload.pct_ct, tx.payload.fee) {
        Ok(_) => Ok(()),
        Err(FeeError::InvalidSelector) => Err(TxAdmissionError::InvalidSelector),
        Err(FeeError::Overflow) => Err(TxAdmissionError::FeeOverflow),
    }
}

impl SignedTransaction {
    pub fn new(
        payload: RawTxPayload,
        public_key: Vec<u8>,
        signature: Vec<u8>,
        lane: FeeLane,
        tip: Option<u64>,
    ) -> Self {
        SignedTransaction {
            payload,
            public_key,
            #[cfg(feature = "quantum")]
            dilithium_public_key: Vec::new(),
            signature: TxSignature {
                ed25519: signature,
                #[cfg(feature = "quantum")]
                dilithium: Vec::new(),
            },
            tip: tip.unwrap_or_default(),
            signer_pubkeys: Vec::new(),
            aggregate_signature: Vec::new(),
            threshold: 0,
            lane,
            version: TxVersion::Ed25519Only,
        }
    }

    fn __repr__(&self) -> String {
        let ed_len = self.signature.ed25519.len();
        #[cfg(feature = "quantum")]
        let pq_len = self.signature.dilithium.len();
        #[cfg(not(feature = "quantum"))]
        let pq_len = 0;
        format!(
            "SignedTransaction(payload={}, public_key=<{} bytes>, signature=<{}|{} bytes>, lane={:?}, tip={})",
            self.payload.__repr__(),
            self.public_key.len(),
            ed_len,
            pq_len,
            self.lane,
            self.tip,
        )
    }
}

/// Blob transaction committing an opaque data blob by root hash.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct BlobTx {
    /// Owner account identifier.
    pub owner: String,
    /// Globally unique blob identifier derived from parent hash and nonce.
    pub blob_id: [u8; 32],
    /// BLAKE3 commitment to the blob contents or erasure-coded shards.
    pub blob_root: [u8; 32],
    /// Total uncompressed size of the blob in bytes.
    pub blob_size: u64,
    /// Fractal layer the blob targets (0=L1,1=L2,2=L3).
    pub fractal_lvl: u8,
    /// Optional expiry epoch after which the blob can be pruned.
    pub expiry: Option<u64>,
}

impl BlobTx {
    pub fn new(
        owner: String,
        blob_id: [u8; 32],
        blob_root: [u8; 32],
        blob_size: u64,
        fractal_lvl: u8,
        expiry: Option<u64>,
    ) -> Self {
        BlobTx {
            owner,
            blob_id,
            blob_root,
            blob_size,
            fractal_lvl,
            expiry,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "BlobTx(owner='{}', blob_id={}, blob_root={}, blob_size={}, fractal_lvl={}, expiry={:?})",
            self.owner,
            crypto_suite::hex::encode(self.blob_id),
            crypto_suite::hex::encode(self.blob_root),
            self.blob_size,
            self.fractal_lvl,
            self.expiry
        )
    }
}

impl SignedTransaction {
    pub fn id(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"TX");
        hasher.update(&[crate::constants::TX_VERSION]);
        let bytes = canonical_payload_bytes(&self.payload);
        hasher.update(&bytes);
        hasher.update(&self.public_key);
        hasher.finalize().into()
    }
}

/// Serialize a [`RawTxPayload`] using the project's canonical binary settings.
pub fn canonical_payload_bytes(payload: &RawTxPayload) -> Vec<u8> {
    encode_raw_payload(payload).unwrap_or_else(|err| panic!("failed to encode raw payload: {err}"))
}

fn canonical_signed_transaction_bytes(tx: &SignedTransaction) -> Vec<u8> {
    encode_signed_transaction(tx)
        .unwrap_or_else(|err| panic!("failed to encode signed transaction: {err}"))
}

/// Determine the shard for a given encoded account address.
pub fn shard_for_address(addr: &str) -> ShardId {
    ledger::address::shard_id(addr)
}

/// Signs a transaction payload with the given Ed25519 private key.
/// Returns `None` if the key length is invalid.
pub fn sign_tx(sk_bytes: &[u8], payload: &RawTxPayload) -> Option<SignedTransaction> {
    let sk_bytes = to_array_32(sk_bytes)?;
    let payload_bytes = canonical_payload_bytes(payload);
    let (sig, public_key) = TX_SIGNER.sign_with_secret(&sk_bytes, &payload_bytes);
    let signature = TxSignature {
        ed25519: sig.to_bytes().to_vec(),
        #[cfg(feature = "quantum")]
        dilithium: Vec::new(),
    };
    Some(SignedTransaction {
        payload: payload.clone(),
        public_key: public_key.to_vec(),
        #[cfg(feature = "quantum")]
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

/// Verifies a signed transaction. Returns `true` if the signature and encoding are valid.
pub fn verify_signed_tx(tx: &SignedTransaction) -> bool {
    let key = {
        let bytes = canonical_signed_transaction_bytes(tx);
        let mut h = Hasher::new();
        h.update(&bytes);
        h.finalize().into()
    };
    if let Some(result) = SIG_CACHE.guard().get(&key).copied() {
        return result;
    }

    let payload_bytes = canonical_payload_bytes(&tx.payload);
    let msg = TX_SIGNER.message(&payload_bytes);
    let res = if !tx.signer_pubkeys.is_empty()
        && !tx.aggregate_signature.is_empty()
        && tx.threshold > 0
    {
        let sigs: Vec<&[u8]> = tx.aggregate_signature.chunks(64).collect();
        if sigs.len() < tx.threshold as usize || sigs.len() != tx.signer_pubkeys.len() {
            false
        } else {
            let mut ok = true;
            for (pk_bytes, sig_bytes) in tx.signer_pubkeys.iter().zip(sigs) {
                let valid = if let (Some(pk), Some(sig_arr)) =
                    (to_array_32(pk_bytes), to_array_64(sig_bytes))
                {
                    if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
                        let sig = Signature::from_bytes(&sig_arr);
                        vk.verify(&msg, &sig).is_ok()
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !valid {
                    ok = false;
                    break;
                }
            }
            ok
        }
    } else {
        match tx.version {
            TxVersion::Ed25519Only => {
                if let (Some(pk), Some(sig_bytes)) = (
                    to_array_32(&tx.public_key),
                    to_array_64(&tx.signature.ed25519),
                ) {
                    if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
                        let sig = Signature::from_bytes(&sig_bytes);
                        vk.verify(&msg, &sig).is_ok()
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            TxVersion::Dual => {
                let ed_ok = if let (Some(pk), Some(sig_bytes)) = (
                    to_array_32(&tx.public_key),
                    to_array_64(&tx.signature.ed25519),
                ) {
                    if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
                        let sig = Signature::from_bytes(&sig_bytes);
                        vk.verify(&msg, &sig).is_ok()
                    } else {
                        false
                    }
                } else {
                    false
                };
                #[cfg(feature = "quantum")]
                let pq_ok =
                    if !tx.dilithium_public_key.is_empty() && !tx.signature.dilithium.is_empty() {
                        dilithium::verify(&tx.dilithium_public_key, &msg, &tx.signature.dilithium)
                    } else {
                        false
                    };
                #[cfg(not(feature = "quantum"))]
                let pq_ok = false;
                ed_ok && pq_ok
            }
            TxVersion::DilithiumOnly => {
                #[cfg(feature = "quantum")]
                {
                    if !tx.dilithium_public_key.is_empty() && !tx.signature.dilithium.is_empty() {
                        dilithium::verify(&tx.dilithium_public_key, &msg, &tx.signature.dilithium)
                    } else {
                        false
                    }
                }
                #[cfg(not(feature = "quantum"))]
                {
                    false
                }
            }
        }
    };
    SIG_CACHE.guard().put(key, res);
    res
}

/// Batch verify a slice of signed transactions, returning per-transaction results.
pub fn verify_signed_txs_batch(txs: &[SignedTransaction]) -> Vec<bool> {
    let mut msgs = Vec::new();
    let mut sigs = Vec::new();
    let mut vks = Vec::new();
    let mut indices = Vec::new();
    for (i, tx) in txs.iter().enumerate() {
        if matches!(tx.version, TxVersion::Ed25519Only | TxVersion::Dual) {
            if let (Some(pk), Some(sig_bytes)) = (
                to_array_32(&tx.public_key),
                to_array_64(&tx.signature.ed25519),
            ) {
                if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
                    let payload_bytes = canonical_payload_bytes(&tx.payload);
                    let msg = TX_SIGNER.message(&payload_bytes);
                    msgs.push(msg);
                    sigs.push(Signature::from_bytes(&sig_bytes));
                    vks.push(vk);
                    indices.push(i);
                }
            }
        }
    }
    let msg_refs: Vec<&[u8]> = msgs.iter().map(|m| m.as_slice()).collect();
    let batch_ok = msg_refs
        .iter()
        .zip(sigs.iter())
        .zip(vks.iter())
        .all(|((msg, sig), vk)| vk.verify_strict(msg, sig).is_ok());
    if batch_ok {
        let mut out = vec![false; txs.len()];
        for &i in &indices {
            out[i] = true;
        }
        out
    } else {
        txs.iter().map(verify_signed_tx).collect()
    }
}

/// Python wrapper for [`sign_tx`]. Raises `ValueError` on invalid key length.
/// Python wrapper for [`sign_tx`], raising ``ValueError`` on key size mismatch.
pub fn sign_tx_py(sk_bytes: Vec<u8>, payload: RawTxPayload) -> PyResult<SignedTransaction> {
    sign_tx(&sk_bytes, &payload).ok_or_else(|| py_value_err("Invalid private key length"))
}

/// Python wrapper for [`verify_signed_tx`].
/// Python wrapper for [`verify_signed_tx`]. Returns ``True`` on success.
pub fn verify_signed_tx_py(tx: SignedTransaction) -> bool {
    verify_signed_tx(&tx)
}

/// Python-accessible canonical payload serializer.
/// Python helper returning canonical bytes for a payload.
pub fn canonical_payload_py(payload: RawTxPayload) -> Vec<u8> {
    canonical_payload_bytes(&payload)
}

/// Decode canonical payload bytes into a :class:`RawTxPayload`.
///
/// Args:
///     bytes (bytes): Serialized payload bytes produced by
///         :func:`canonical_payload`.
///
/// Returns:
///     RawTxPayload: Decoded payload structure.
///
/// Raises:
///     ValueError: If ``bytes`` cannot be deserialized.
pub fn decode_payload_py(bytes: Vec<u8>) -> PyResult<RawTxPayload> {
    decode_raw_payload(&bytes).map_err(|e| py_value_err(format!("decode: {e}")))
}

// First-party transaction tests (no third-party dependencies)
#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::ed25519::SIGNATURE_LENGTH;

    fn sample_payload() -> RawTxPayload {
        RawTxPayload {
            from_: "alice".to_string(),
            to: "bob".to_string(),
            amount_consumer: 1,
            amount_industrial: 0,
            fee: 100,
            pct_ct: 100,
            nonce: 1,
            memo: Vec::new(),
        }
    }

    #[test]
    fn sign_tx_roundtrip_verifies_with_suite_types() {
        let payload = sample_payload();
        let sk = [7u8; crypto_suite::signatures::ed25519::SECRET_KEY_LENGTH];
        let signed = sign_tx(&sk, &payload).expect("tx signed");

        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(&signed.public_key);
        let verifying_key = VerifyingKey::from_bytes(&pk_bytes).expect("verifying key");

        let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(&signed.signature.ed25519);
        let signature = Signature::from_bytes(&sig_bytes);

        let payload_bytes = canonical_payload_bytes(&payload);
        let msg = TX_SIGNER.message(&payload_bytes);
        verifying_key
            .verify(&msg, &signature)
            .expect("suite verification");
        assert!(verify_signed_tx(&signed));
    }

    #[test]
    fn signed_transaction_new_handles_optional_tip() {
        let payload = sample_payload();
        let public_key = vec![1u8; 32];
        let signature = vec![2u8; 64];

        // Test without tip (None)
        let tx = SignedTransaction::new(
            payload.clone(),
            public_key.clone(),
            signature.clone(),
            FeeLane::Consumer,
            None,
        );
        assert_eq!(tx.tip, 0);
        assert_eq!(tx.public_key, public_key);
        assert_eq!(tx.signature.ed25519, signature);

        // Test with tip (Some)
        let tx_with_tip =
            SignedTransaction::new(payload, public_key, signature, FeeLane::Consumer, Some(42));
        assert_eq!(tx_with_tip.tip, 42);
    }

    #[test]
    fn canonical_payload_bytes_deterministic() {
        let payload = sample_payload();
        let bytes1 = canonical_payload_bytes(&payload);
        let bytes2 = canonical_payload_bytes(&payload);
        assert_eq!(bytes1, bytes2, "canonical encoding must be deterministic");
    }

    #[test]
    fn tx_signature_roundtrip() {
        let sig = TxSignature {
            ed25519: vec![1, 2, 3, 4],
            #[cfg(feature = "quantum")]
            dilithium: vec![5, 6, 7, 8],
        };

        // Verify fields are set correctly
        assert_eq!(sig.ed25519, vec![1, 2, 3, 4]);
        #[cfg(feature = "quantum")]
        assert_eq!(sig.dilithium, vec![5, 6, 7, 8]);
    }

    #[test]
    fn tx_version_variants() {
        // Test all TxVersion variants exist
        let _ = TxVersion::Ed25519Only;
        #[cfg(feature = "quantum")]
        let _ = TxVersion::DilithiumOnly;
        let _ = TxVersion::Dual;
    }

    #[test]
    fn fee_lane_variants() {
        // Test all FeeLane variants exist and are distinct
        assert_ne!(
            format!("{:?}", FeeLane::Consumer),
            format!("{:?}", FeeLane::Industrial),
            "Consumer and Industrial lanes should be distinct"
        );
    }

    #[test]
    fn verify_signed_tx_rejects_invalid_signature() {
        let payload = sample_payload();
        let sk = [7u8; crypto_suite::signatures::ed25519::SECRET_KEY_LENGTH];
        let mut signed = sign_tx(&sk, &payload).expect("tx signed");

        // Corrupt the signature
        signed.signature.ed25519[0] ^= 0xFF;

        assert!(!verify_signed_tx(&signed), "corrupted signature should fail verification");
    }
}
