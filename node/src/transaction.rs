//! Transaction data structures and signing utilities.
//!
//! Exposes Python bindings for constructing, signing, and verifying
//! transactions using Ed25519 with domain separation.

use crate::{constants::bincode_config, constants::domain_tag, to_array_32, to_array_64};
use bincode::Options;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hex;
use serde::{Deserialize, Serialize};

use crate::{fee, fee::FeeError, TxAdmissionError};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
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

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    #[pyo3(get, set)]
    pub payload: RawTxPayload,
    #[pyo3(get, set)]
    pub public_key: Vec<u8>,
    #[pyo3(get, set)]
    pub signature: Vec<u8>,
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
            signature,
            signer_pubkeys: Vec::new(),
            aggregate_signature: Vec::new(),
            threshold: 0,
            lane,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SignedTransaction(payload={}, public_key=<{} bytes>, signature=<{} bytes>, lane={:?})",
            self.payload.__repr__(),
            self.public_key.len(),
            self.signature.len(),
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
    Some(SignedTransaction {
        payload: payload.clone(),
        public_key: sk.verifying_key().to_bytes().to_vec(),
        signature: sig.to_bytes().to_vec(),
        lane: FeeLane::Consumer,
    })
}

/// Verifies a signed transaction. Returns `true` if the signature and encoding are valid.
pub fn verify_signed_tx(tx: &SignedTransaction) -> bool {
    if !tx.signer_pubkeys.is_empty() && !tx.aggregate_signature.is_empty() && tx.threshold > 0 {
        let sigs: Vec<&[u8]> = tx.aggregate_signature.chunks(64).collect();
        if sigs.len() < tx.threshold as usize || sigs.len() != tx.signer_pubkeys.len() {
            return false;
        }
        for (pk_bytes, sig_bytes) in tx.signer_pubkeys.iter().zip(sigs) {
            if let (Some(pk), Some(sig_arr)) = (to_array_32(pk_bytes), to_array_64(sig_bytes)) {
                if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
                    let mut m = domain_tag().to_vec();
                    m.extend(canonical_payload_bytes(&tx.payload));
                    let sig = Signature::from_bytes(&sig_arr);
                    if vk.verify(&m, &sig).is_err() {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        return true;
    }
    if let (Some(pk), Some(sig_bytes)) = (to_array_32(&tx.public_key), to_array_64(&tx.signature)) {
        if let Ok(vk) = VerifyingKey::from_bytes(&pk) {
            let mut m = domain_tag().to_vec();
            m.extend(canonical_payload_bytes(&tx.payload));
            let sig = Signature::from_bytes(&sig_bytes);
            return vk.verify(&m, &sig).is_ok();
        }
    }
    false
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
