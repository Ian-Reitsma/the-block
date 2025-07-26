use crate::{constants::bincode_config, constants::domain_tag, to_array_32, to_array_64};
use bincode::Options;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use pyo3::prelude::*;

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct RawTxPayload {
    #[pyo3(get, set, name = "from")]
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
    pub fee_token: u8,
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
        fee_token: u8,
        nonce: u64,
        memo: Vec<u8>,
    ) -> Self {
        RawTxPayload {
            from_,
            to,
            amount_consumer,
            amount_industrial,
            fee,
            fee_token,
            nonce,
            memo,
        }
    }
    fn __repr__(&self) -> String {
        format!(
            "RawTxPayload(from='{}', to='{}', amount_consumer={}, amount_industrial={}, fee={}, fee_token={}, nonce={}, memo_len={})",
            self.from_,
            self.to,
            self.amount_consumer,
            self.amount_industrial,
            self.fee,
            self.fee_token,
            self.nonce,
            self.memo.len(),
        )
    }
}

#[pyclass]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct SignedTransaction {
    #[pyo3(get, set)]
    pub payload: RawTxPayload,
    #[pyo3(get, set)]
    pub public_key: Vec<u8>,
    #[pyo3(get, set)]
    pub signature: Vec<u8>,
}

#[pymethods]
impl SignedTransaction {
    #[new]
    pub fn new(payload: RawTxPayload, public_key: Vec<u8>, signature: Vec<u8>) -> Self {
        SignedTransaction {
            payload,
            public_key,
            signature,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SignedTransaction(payload={}, public_key=<{} bytes>, signature=<{} bytes>)",
            self.payload.__repr__(),
            self.public_key.len(),
            self.signature.len(),
        )
    }
}

impl SignedTransaction {
    pub fn id(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"TX");
        let bytes = canonical_payload_bytes(&self.payload);
        hasher.update(&bytes);
        hasher.update(&self.public_key);
        hasher.finalize().into()
    }
}

pub fn canonical_payload_bytes(payload: &RawTxPayload) -> Vec<u8> {
    bincode_config().serialize(payload).unwrap()
}

pub fn sign_tx(sk_bytes: &[u8], payload: &RawTxPayload) -> SignedTransaction {
    let sk = SigningKey::from_bytes(&to_array_32(sk_bytes));
    let msg = {
        let mut m = domain_tag().to_vec();
        m.extend(canonical_payload_bytes(payload));
        m
    };
    let sig = sk.sign(&msg);
    SignedTransaction {
        payload: payload.clone(),
        public_key: sk.verifying_key().to_bytes().to_vec(),
        signature: sig.to_bytes().to_vec(),
    }
}

pub fn verify_signed_tx(tx: &SignedTransaction) -> bool {
    if let Ok(vk) = VerifyingKey::from_bytes(&to_array_32(&tx.public_key)) {
        let mut m = domain_tag().to_vec();
        m.extend(canonical_payload_bytes(&tx.payload));
        let sig = Signature::from_bytes(&to_array_64(&tx.signature));
        vk.verify(&m, &sig).is_ok()
    } else {
        false
    }
}

#[pyfunction(name = "sign_tx")]
pub fn sign_tx_py(sk_bytes: Vec<u8>, payload: RawTxPayload) -> SignedTransaction {
    sign_tx(&sk_bytes, &payload)
}

#[pyfunction(name = "verify_signed_tx")]
pub fn verify_signed_tx_py(tx: SignedTransaction) -> bool {
    verify_signed_tx(&tx)
}