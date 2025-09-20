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
use std::fmt;
use std::num::NonZeroUsize;

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

impl<'py> IntoPyObject<'py> for TxSignature {
    type Target = PyDict;
    type Output = Bound<'py, PyDict>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        let dict = PyDict::new(py);
        dict.set_item("ed25519", PyBytes::new(py, &self.ed25519))?;
        #[cfg(feature = "quantum")]
        {
            dict.set_item("dilithium", PyBytes::new(py, &self.dilithium))?;
        }
        Ok(dict)
    }
}

impl<'py> FromPyObject<'py> for TxSignature {
    fn extract_bound(obj: &Bound<'py, PyAny>) -> PyResult<Self> {
        if let Ok(dict) = obj.downcast::<PyDict>() {
            let ed25519 = dict
                .get_item("ed25519")?
                .map(|value| value.extract::<Vec<u8>>())
                .transpose()?
                .unwrap_or_default();
            #[cfg(feature = "quantum")]
            {
                let dilithium = dict
                    .get_item("dilithium")?
                    .map(|value| value.extract::<Vec<u8>>())
                    .transpose()?
                    .unwrap_or_default();
                return Ok(TxSignature { ed25519, dilithium });
            }
            #[cfg(not(feature = "quantum"))]
            {
                return Ok(TxSignature { ed25519 });
            }
        }

        if let Ok(ed25519) = obj.extract::<Vec<u8>>() {
            #[cfg(feature = "quantum")]
            {
                return Ok(TxSignature {
                    ed25519,
                    dilithium: Vec::new(),
                });
            }
            #[cfg(not(feature = "quantum"))]
            {
                return Ok(TxSignature { ed25519 });
            }
        }

        if let Ok(ed_attr) = obj.getattr("ed25519") {
            let ed25519 = ed_attr.extract::<Vec<u8>>()?;
            #[cfg(feature = "quantum")]
            {
                let dilithium = obj
                    .getattr_opt("dilithium")?
                    .map(|value| value.extract::<Vec<u8>>())
                    .transpose()?
                    .unwrap_or_default();
                return Ok(TxSignature { ed25519, dilithium });
            }
            #[cfg(not(feature = "quantum"))]
            {
                return Ok(TxSignature { ed25519 });
            }
        }

        Err(PyTypeError::new_err(
            "TxSignature must be bytes or a mapping/object with an 'ed25519' attribute",
        ))
    }
}

impl<'py> IntoPyObject<'py> for TxVersion {
    type Target = PyString;
    type Output = Bound<'py, PyString>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        let value = match self {
            TxVersion::Ed25519Only => "ed25519_only",
            TxVersion::Dual => "dual",
            TxVersion::DilithiumOnly => "dilithium_only",
        };
        Ok(PyString::new(py, value))
    }
}

impl<'py> FromPyObject<'py> for TxVersion {
    fn extract_bound(obj: &Bound<'py, PyAny>) -> PyResult<Self> {
        if let Ok(name) = obj.extract::<&str>() {
            let normalized = name.replace('-', "_").to_ascii_lowercase();
            return match normalized.as_str() {
                "ed25519_only" | "ed25519" | "ed25519only" => Ok(TxVersion::Ed25519Only),
                "dual" => Ok(TxVersion::Dual),
                "dilithium_only" | "dilithium" | "dilithiumonly" => Ok(TxVersion::DilithiumOnly),
                other => Err(PyValueError::new_err(format!(
                    "invalid TxVersion string: {other}"
                ))),
            };
        }

        if let Ok(value) = obj.extract::<u8>() {
            return match value {
                0 => Ok(TxVersion::Ed25519Only),
                1 => Ok(TxVersion::Dual),
                2 => Ok(TxVersion::DilithiumOnly),
                other => Err(PyValueError::new_err(format!(
                    "invalid TxVersion value: {other}"
                ))),
            };
        }

        Err(PyTypeError::new_err(
            "TxVersion must be specified as a string (e.g. 'dual') or integer (0, 1, 2)",
        ))
    }
}

use crate::{fee, fee::FeeError, TxAdmissionError};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyAnyMethods, PyBytes, PyDict, PyDictMethods, PyString};
use pyo3::Bound;
use pyo3::PyErr;

static SIG_CACHE: Lazy<Mutex<LruCache<[u8; 32], bool>>> = Lazy::new(|| {
    Mutex::new(LruCache::new(
        NonZeroUsize::new(1024).expect("cache size non-zero"),
    ))
});

/// Remote attestation accompanying a DID anchor transaction.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchorAttestation {
    /// Hex-encoded verifying key for the remote signer.
    #[serde(default)]
    pub signer: String,
    /// Hex-encoded signature over the attestation message.
    #[serde(default)]
    pub signature: String,
}

/// Specialized transaction anchoring a DID document hash on-chain.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct TxDidAnchor {
    /// Account address owning the DID document.
    #[serde(default)]
    pub address: String,
    /// Public key authorizing the update.
    #[serde(default)]
    pub public_key: Vec<u8>,
    /// Canonical DID document body.
    #[serde(default)]
    pub document: String,
    /// Monotonic nonce protecting against replay.
    #[serde(default)]
    pub nonce: u64,
    /// Ed25519 signature from the owner over the anchor digest.
    #[serde(default)]
    pub signature: Vec<u8>,
    /// Optional remote attestation signed by a provenance-configured key.
    #[serde(default)]
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
#[pyclass]
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
    #[pyo3(signature = (payload, public_key, signature, lane, tip=None))]
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
    let mut msg = Vec::with_capacity(domain.len() + payload_bytes.len());
    msg.extend_from_slice(domain);
    msg.extend_from_slice(&payload_bytes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::{
        types::{IntoPyDict, PyBytes, PyDict, PyModule},
        Py,
    };
    use std::ffi::CString;

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
    fn signed_transaction_new_handles_optional_tip() {
        let payload = sample_payload();
        let public_key = vec![1u8; 32];
        let signature = vec![2u8; 64];
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

        let tx_with_tip =
            SignedTransaction::new(payload, public_key, signature, FeeLane::Consumer, Some(42));
        assert_eq!(tx_with_tip.tip, 42);
    }

    #[test]
    fn python_constructor_supports_tip_keyword() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let payload = Py::new(py, sample_payload()).expect("payload object");
            let lane = Py::new(py, FeeLane::Consumer).expect("lane object");
            let tx_type = py.get_type::<SignedTransaction>();
            let tx_default = tx_type
                .call1((
                    payload.clone_ref(py),
                    Vec::<u8>::new(),
                    vec![1u8; 16],
                    lane.clone_ref(py),
                ))
                .expect("default constructor call");
            assert_eq!(
                tx_default
                    .getattr("tip")
                    .expect("tip attr")
                    .extract::<u64>()
                    .expect("tip extract"),
                0
            );

            let payload_kw = Py::new(py, sample_payload()).expect("payload kw");
            let lane_kw = Py::new(py, FeeLane::Consumer).expect("lane kw");
            let kwargs = [("tip", 99u64)].into_py_dict(py).expect("kwargs dict");
            let tx_kw = tx_type
                .call(
                    (payload_kw, Vec::<u8>::new(), vec![1u8; 16], lane_kw),
                    Some(&kwargs),
                )
                .expect("keyword constructor call");
            assert_eq!(
                tx_kw
                    .getattr("tip")
                    .expect("tip attr")
                    .extract::<u64>()
                    .expect("tip extract"),
                99
            );
        });
    }

    #[test]
    fn tx_signature_extracts_from_dict() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let dict = PyDict::new(py);
            dict.set_item("ed25519", PyBytes::new(py, &[1, 2, 3]))
                .expect("set ed25519");
            #[cfg(feature = "quantum")]
            dict.set_item("dilithium", PyBytes::new(py, &[4, 5, 6]))
                .expect("set dilithium");

            let sig: TxSignature = dict.extract().expect("dict extract");
            assert_eq!(sig.ed25519, vec![1, 2, 3]);
            #[cfg(feature = "quantum")]
            assert_eq!(sig.dilithium, vec![4, 5, 6]);
        });
    }

    #[test]
    fn tx_signature_extracts_from_bytes() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let bytes = PyBytes::new(py, &[9, 8, 7]);
            let sig: TxSignature = bytes.extract().expect("bytes extract");
            assert_eq!(sig.ed25519, vec![9, 8, 7]);
            #[cfg(feature = "quantum")]
            assert!(sig.dilithium.is_empty());
        });
    }

    #[test]
    fn tx_signature_extracts_from_attributes() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            #[cfg(feature = "quantum")]
            let module = {
                let code = CString::new(
                    "class Holder:\n    def __init__(self):\n        self.ed25519 = b'abc'\n        self.dilithium = b'def'\nholder = Holder()\n",
                )
                .expect("code CString");
                PyModule::from_code(
                    py,
                    code.as_c_str(),
                    pyo3::ffi::c_str!(""),
                    pyo3::ffi::c_str!("holder_module"),
                )
                .expect("module")
            };
            #[cfg(not(feature = "quantum"))]
            let module = {
                let code = CString::new(
                    "class Holder:\n    def __init__(self):\n        self.ed25519 = b'abc'\nholder = Holder()\n",
                )
                .expect("code CString");
                PyModule::from_code(
                    py,
                    code.as_c_str(),
                    pyo3::ffi::c_str!(""),
                    pyo3::ffi::c_str!("holder_module"),
                )
                .expect("module")
            };

            let holder = module.getattr("holder").expect("holder attr");
            let sig: TxSignature = holder.extract().expect("attr extract");
            assert_eq!(sig.ed25519, b"abc".to_vec());
            #[cfg(feature = "quantum")]
            assert_eq!(sig.dilithium, b"def".to_vec());
        });
    }

    #[test]
    fn tx_version_extracts_from_strings() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let py_str = py
                .eval(pyo3::ffi::c_str!("'Dual'"), None, None)
                .expect("eval dual");
            let version: TxVersion = py_str.extract().expect("string extract");
            assert_eq!(version, TxVersion::Dual);

            let ed = py
                .eval(pyo3::ffi::c_str!("'ed25519-only'"), None, None)
                .expect("eval ed25519");
            let version: TxVersion = ed.extract().expect("normalize ed");
            assert_eq!(version, TxVersion::Ed25519Only);
        });
    }

    #[test]
    fn tx_version_extracts_from_integers() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let zero = py
                .eval(pyo3::ffi::c_str!("0"), None, None)
                .expect("zero eval");
            let version: TxVersion = zero.extract().expect("zero extract");
            assert_eq!(version, TxVersion::Ed25519Only);

            let two = py
                .eval(pyo3::ffi::c_str!("2"), None, None)
                .expect("two eval");
            let version: TxVersion = two.extract().expect("two extract");
            assert_eq!(version, TxVersion::DilithiumOnly);
        });
    }
}
