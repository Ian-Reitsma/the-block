//! Transaction data structures and signing utilities.
//!
//! Exposes Python bindings for constructing, signing, and verifying
//! transactions using Ed25519 with domain separation.

use crate::{constants::bincode_config, constants::domain_tag, to_array_32, to_array_64};
use bincode::Options;
use blake3::Hasher;
#[cfg(feature = "quantum")]
use crypto::dilithium;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hex;
use ledger::address::{self, ShardId};
use lru::LruCache;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum TxVersion {
    Ed25519Only,
    Dual,
    DilithiumOnly,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct TxSignature {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ed25519: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

use std::cell::RefCell;

use crate::{fee, fee::FeeError, TxAdmissionError};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

thread_local! {
    static MSG_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

static SIG_CACHE: Lazy<Mutex<LruCache<[u8; 32], bool>>> =
    Lazy::new(|| Mutex::new(LruCache::new(1024)));

/// Distinct fee lanes for transaction scheduling.
#[pyclass]
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
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

impl Default for FeeLane {
    fn default() -> Self {
        FeeLane::Consumer
    }
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct RawTxPayload {
    #[pyo3(get, set)]
    pub from_: String,
    #[pyo3(get, set)]
    pub to: String,
    #[pyo3(get, set)]
    pub amount_consumer: u64,
    #[pyo3(get, set)]
    pub amount_industrial: u64,
    #[pyo3(get, set)]
    pub fee: u64,
    #[pyo3(get, set)]
    pub pct_ct: u8,
    #[pyo3(get, set)]
    pub nonce: u64,
    #[pyo3(get, set)]
    pub memo: Vec<u8>,
}

#[pymethods]
impl RawTxPayload {
    #[new]
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
    #[getter(from)]
    fn get_from_alias(&self) -> String {
        self.from_.clone()
    }

    #[setter(from)]
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

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    #[pyo3(get, set)]
    pub payload: RawTxPayload,
    #[pyo3(get, set)]
    pub public_key: Vec<u8>,
    #[cfg(feature = "quantum")]
    #[pyo3(get, set)]
    #[serde(default)]
    pub dilithium_public_key: Vec<u8>,
    #[pyo3(get, set)]
    pub signature: TxSignature,
    /// Priority fee paid to the miner above the base fee.
    #[pyo3(get, set)]
    #[serde(default)]
    pub tip: u64,
    /// Optional set of signer public keys for multisig.
    #[pyo3(get, set)]
    #[serde(default)]
    pub signer_pubkeys: Vec<Vec<u8>>,
    /// Aggregated signatures concatenated in order.
    #[pyo3(get, set)]
    #[serde(default)]
    pub aggregate_signature: Vec<u8>,
    /// Required number of signatures.
    #[pyo3(get, set)]
    #[serde(default)]
    pub threshold: u8,
    /// Fee lane classification for admission and scheduling.
    #[pyo3(get, set)]
    #[serde(default)]
    pub lane: FeeLane,
    /// Signature mode for the transaction.
    #[pyo3(get, set)]
    #[serde(default)]
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

#[pymethods]
impl SignedTransaction {
    #[new]
    pub fn new(
        payload: RawTxPayload,
        public_key: Vec<u8>,
        signature: Vec<u8>,
        lane: FeeLane,
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
            "SignedTransaction(payload={}, public_key=<{} bytes>, signature=<{}|{} bytes>, lane={:?})",
            self.payload.__repr__(),
            self.public_key.len(),
            ed_len,
            pq_len,
            self.lane,
        )
    }
}

/// Blob transaction committing an opaque data blob by root hash.
#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct BlobTx {
    /// Owner account identifier.
    #[pyo3(get, set)]
    pub owner: String,
    /// Globally unique blob identifier derived from parent hash and nonce.
    #[pyo3(get, set)]
    pub blob_id: [u8; 32],
    /// BLAKE3 commitment to the blob contents or erasure-coded shards.
    #[pyo3(get, set)]
    pub blob_root: [u8; 32],
    /// Total uncompressed size of the blob in bytes.
    #[pyo3(get, set)]
    pub blob_size: u64,
    /// Fractal layer the blob targets (0=L1,1=L2,2=L3).
    #[pyo3(get, set)]
    pub fractal_lvl: u8,
    /// Optional expiry epoch after which the blob can be pruned.
    #[pyo3(get, set)]
    pub expiry: Option<u64>,
}

#[pymethods]
impl BlobTx {
    #[new]
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
            hex::encode(self.blob_id),
            hex::encode(self.blob_root),
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

/// Serialize a [`RawTxPayload`] using the project's canonical bincode settings.
pub fn canonical_payload_bytes(payload: &RawTxPayload) -> Vec<u8> {
    bincode_config()
        .serialize(payload)
        .unwrap_or_else(|e| panic!("serialize: {e}"))
}

/// Determine the shard for a given encoded account address.
pub fn shard_for_address(addr: &str) -> ShardId {
    ledger::address::shard_id(addr)
}

/// Signs a transaction payload with the given Ed25519 private key.
/// Returns `None` if the key length is invalid.
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
        #[cfg(feature = "quantum")]
        dilithium: Vec::new(),
    };
    Some(SignedTransaction {
        payload: payload.clone(),
        public_key: sk.verifying_key().to_bytes().to_vec(),
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
        let bytes = bincode::serialize(tx).unwrap_or_default();
        let mut h = Hasher::new();
        h.update(&bytes);
        h.finalize().into()
    };
    if let Some(result) = SIG_CACHE.lock().get(&key).copied() {
        return result;
    }

    let payload_bytes = canonical_payload_bytes(&tx.payload);
    let domain = domain_tag();
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
                        MSG_BUF.with(|buf| {
                            let mut buf = buf.borrow_mut();
                            buf.clear();
                            buf.extend_from_slice(domain);
                            buf.extend_from_slice(&payload_bytes);
                            let sig = Signature::from_bytes(&sig_arr);
                            vk.verify(&buf, &sig).is_ok()
                        })
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
                        MSG_BUF.with(|buf| {
                            let mut buf = buf.borrow_mut();
                            buf.clear();
                            buf.extend_from_slice(domain);
                            buf.extend_from_slice(&payload_bytes);
                            let sig = Signature::from_bytes(&sig_bytes);
                            vk.verify(&buf, &sig).is_ok()
                        })
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
                        MSG_BUF.with(|buf| {
                            let mut buf = buf.borrow_mut();
                            buf.clear();
                            buf.extend_from_slice(domain);
                            buf.extend_from_slice(&payload_bytes);
                            let sig = Signature::from_bytes(&sig_bytes);
                            vk.verify(&buf, &sig).is_ok()
                        })
                    } else {
                        false
                    }
                } else {
                    false
                };
                #[cfg(feature = "quantum")]
                let pq_ok =
                    if !tx.dilithium_public_key.is_empty() && !tx.signature.dilithium.is_empty() {
                        let mut msg = domain.to_vec();
                        msg.extend_from_slice(&payload_bytes);
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
                        let mut msg = domain.to_vec();
                        msg.extend_from_slice(&payload_bytes);
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
    SIG_CACHE.lock().put(key, res);
    res
}

/// Batch verify a slice of signed transactions, returning per-transaction results.
pub fn verify_signed_txs_batch(txs: &[SignedTransaction]) -> Vec<bool> {
    use ed25519_dalek::Signature as DalekSig;
    let domain = domain_tag();
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
                    let mut msg = Vec::with_capacity(domain.len() + payload_bytes.len());
                    msg.extend_from_slice(domain);
                    msg.extend_from_slice(&payload_bytes);
                    msgs.push(msg);
                    sigs.push(DalekSig::from_bytes(&sig_bytes));
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
#[pyfunction(name = "sign_tx")]
/// Python wrapper for [`sign_tx`], raising ``ValueError`` on key size mismatch.
pub fn sign_tx_py(sk_bytes: Vec<u8>, payload: RawTxPayload) -> PyResult<SignedTransaction> {
    sign_tx(&sk_bytes, &payload).ok_or_else(|| PyValueError::new_err("Invalid private key length"))
}

/// Python wrapper for [`verify_signed_tx`].
#[pyfunction(name = "verify_signed_tx")]
/// Python wrapper for [`verify_signed_tx`]. Returns ``True`` on success.
pub fn verify_signed_tx_py(tx: SignedTransaction) -> bool {
    verify_signed_tx(&tx)
}

/// Python-accessible canonical payload serializer.
#[pyfunction(name = "canonical_payload")]
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
#[pyfunction(name = "decode_payload", text_signature = "(bytes)")]
pub fn decode_payload_py(bytes: Vec<u8>) -> PyResult<RawTxPayload> {
    bincode_config()
        .deserialize(&bytes)
        .map_err(|e| PyValueError::new_err(format!("decode: {e}")))
}
