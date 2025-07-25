use crate::{to_array_32, to_array_64};
use bincode;
use bincode::Options;
use blake3;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

const DOMAIN_TAG: &[u8] = b"THE_BLOCK|v1|";

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
}

#[pymethods]
impl RawTxPayload {
    #[new]
    pub fn new(
        from_: String,
        to: String,
        amount_consumer: u64,
        amount_industrial: u64,
        fee: u64,
    ) -> Self {
        RawTxPayload {
            from_,
            to,
            amount_consumer,
            amount_industrial,
            fee,
        }
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
    pub fn new(
        payload: RawTxPayload,
        public_key: Vec<u8>,
        signature: Vec<u8>,
    ) -> Self {
        SignedTransaction {
            payload,
            public_key,
            signature,
        }
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
    let conf = bincode::DefaultOptions::new()
        .with_little_endian()
        .with_fixint_encoding();
    conf.serialize(payload).unwrap()
}

pub fn sign_tx(sk_bytes: &[u8], payload: &RawTxPayload) -> SignedTransaction {
    let sk = SigningKey::from_bytes(&to_array_32(sk_bytes));
    let msg = {
        let mut m = DOMAIN_TAG.to_vec();
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
        let mut m = DOMAIN_TAG.to_vec();
        m.extend(canonical_payload_bytes(&tx.payload));
        let sig = Signature::from_bytes(&to_array_64(&tx.signature));
        vk.verify(&m, &sig).is_ok()
    } else {
        false
    }
}

#[pyfunction]
pub fn py_sign_tx(sk_bytes: Vec<u8>, payload: RawTxPayload) -> SignedTransaction {
    sign_tx(&sk_bytes, &payload)
}

#[pyfunction]
pub fn py_verify_signed_tx(tx: SignedTransaction) -> bool {
    verify_signed_tx(&tx)
}