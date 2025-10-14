use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crypto_suite::hashing::blake3::hash;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{Deserialize, Serialize};
use ledger::address;

use crate::{
    governance::GovStore,
    identity::did_binary,
    provenance,
    simple_db::names,
    to_array_32, to_array_64,
    transaction::{TxDidAnchor, TxDidAnchorAttestation},
    SimpleDb,
};
use state::{DidState, DidStateError};

#[cfg(feature = "telemetry")]
use crate::telemetry::DID_ANCHOR_TOTAL;

const MAX_DID_DOC_BYTES: usize = 64 * 1024;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn did_key(address: &str) -> String {
    format!("did/{address}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DidAttestationRecord {
    pub signer: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DidRecord {
    pub address: String,
    pub document: String,
    pub hash: [u8; 32],
    pub nonce: u64,
    pub updated_at: u64,
    pub public_key: Vec<u8>,
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub remote_attestation: Option<DidAttestationRecord>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DidError {
    InvalidAddress,
    InvalidKey,
    BadSignature,
    Replay,
    Storage,
    Revoked,
    DocumentTooLarge,
    InvalidAttestation,
    UnknownRemoteSigner,
    InvalidRequest,
}

impl DidError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            DidError::InvalidAddress => "E_DID_ADDR",
            DidError::InvalidKey => "E_DID_KEY",
            DidError::BadSignature => "E_DID_SIG",
            DidError::Replay => "E_DID_REPLAY",
            DidError::Storage => "E_DID_STORAGE",
            DidError::Revoked => "E_DID_REVOKED",
            DidError::DocumentTooLarge => "E_DID_DOC_TOO_LARGE",
            DidError::InvalidAttestation => "E_DID_ATTEST",
            DidError::UnknownRemoteSigner => "E_DID_SIGNER",
            DidError::InvalidRequest => "E_DID_REQUEST",
        }
    }
}

pub struct DidRegistry {
    db: SimpleDb,
}

impl DidRegistry {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let path_str = path.as_ref().to_string_lossy().to_string();
        Self {
            db: SimpleDb::open_named(names::IDENTITY_DID, &path_str),
        }
    }

    #[must_use]
    pub fn default_path() -> String {
        std::env::var("TB_DID_DB_PATH").unwrap_or_else(|_| "identity_did_db".into())
    }

    #[must_use]
    pub fn addresses(&self) -> Vec<String> {
        self.db
            .keys_with_prefix("did/")
            .into_iter()
            .filter_map(|key| key.strip_prefix("did/").map(|s| s.to_string()))
            .collect()
    }

    #[must_use]
    pub fn records(&self) -> Vec<DidRecord> {
        self.addresses()
            .into_iter()
            .filter_map(|addr| self.resolve(&addr))
            .collect()
    }

    fn validate_owner(tx: &TxDidAnchor) -> Result<VerifyingKey, DidError> {
        if tx.public_key.len() != 32 {
            return Err(DidError::InvalidKey);
        }
        let pk_bytes = to_array_32(&tx.public_key).ok_or(DidError::InvalidKey)?;
        let verifying_key =
            VerifyingKey::from_bytes(&pk_bytes).map_err(|_| DidError::InvalidKey)?;
        let account_part = address::account(&tx.address);
        let pk_hex = crypto_suite::hex::encode(pk_bytes);
        if tx.address != pk_hex && account_part != pk_hex {
            return Err(DidError::InvalidAddress);
        }
        let sig = to_array_64(&tx.signature).ok_or(DidError::BadSignature)?;
        let signature = Signature::from_bytes(&sig);
        verifying_key
            .verify(tx.owner_digest().as_ref(), &signature)
            .map_err(|_| DidError::BadSignature)?;
        Ok(verifying_key)
    }

    fn validate_attestation(
        tx: &TxDidAnchor,
        att: &TxDidAnchorAttestation,
    ) -> Result<DidAttestationRecord, DidError> {
        if att.signer.trim().is_empty() || att.signature.trim().is_empty() {
            return Err(DidError::InvalidAttestation);
        }
        let signer_hex = att.signer.trim().to_lowercase();
        let configured = provenance::release_signer_hexes();
        if !configured.iter().any(|s| s == &signer_hex) {
            return Err(DidError::UnknownRemoteSigner);
        }
        let signer_vk =
            provenance::parse_signer_hex(&signer_hex).ok_or(DidError::InvalidAttestation)?;
        let sig_bytes = crypto_suite::hex::decode(att.signature.trim())
            .map_err(|_| DidError::InvalidAttestation)?;
        if sig_bytes.len() != 64 {
            return Err(DidError::InvalidAttestation);
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&arr);
        signer_vk
            .verify(tx.remote_digest().as_ref(), &signature)
            .map_err(|_| DidError::InvalidAttestation)?;
        Ok(DidAttestationRecord {
            signer: signer_hex,
            signature: att.signature.trim().to_lowercase(),
        })
    }

    pub fn anchor(
        &mut self,
        tx: &TxDidAnchor,
        gov: Option<&GovStore>,
    ) -> Result<DidRecord, DidError> {
        if tx.document.len() > MAX_DID_DOC_BYTES {
            return Err(DidError::DocumentTooLarge);
        }
        if let Some(store) = gov {
            if store.is_did_revoked(&tx.address) {
                return Err(DidError::Revoked);
            }
        }
        Self::validate_owner(tx)?;
        let remote_attestation = if let Some(att) = &tx.remote_attestation {
            Some(Self::validate_attestation(tx, att)?)
        } else {
            None
        };

        let hash = tx.document_hash();
        let ts = now_secs();

        let prev = self
            .db
            .get(&did_key(&tx.address))
            .and_then(|raw| did_binary::decode_record(&raw).ok());
        let mut state = prev
            .as_ref()
            .map(|rec| DidState {
                hash: rec.hash,
                nonce: rec.nonce,
                updated_at: rec.updated_at,
            })
            .unwrap_or_default();
        match state.apply_update(tx.nonce, hash, ts) {
            Ok(()) => {}
            Err(DidStateError::Replay) => return Err(DidError::Replay),
        }

        let record = DidRecord {
            address: tx.address.clone(),
            document: tx.document.clone(),
            hash,
            nonce: tx.nonce,
            updated_at: ts,
            public_key: tx.public_key.clone(),
            remote_attestation,
        };
        let bytes = did_binary::encode_record(&record);
        self.db.insert(&did_key(&record.address), bytes);

        #[cfg(feature = "telemetry")]
        DID_ANCHOR_TOTAL.inc();

        Ok(record)
    }

    pub fn resolve(&self, address: &str) -> Option<DidRecord> {
        self.db
            .get(&did_key(address))
            .and_then(|raw| did_binary::decode_record(&raw).ok())
    }
}

/// Convenience helper retaining backwards-compatible hashing semantics.
#[must_use]
pub fn anchor(doc: &str) -> [u8; 32] {
    let h = hash(doc.as_bytes()).into();
    #[cfg(feature = "telemetry")]
    DID_ANCHOR_TOTAL.inc();
    h
}
