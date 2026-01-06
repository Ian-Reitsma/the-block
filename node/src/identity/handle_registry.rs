use crate::{identity::handle_binary, simple_db::names, SimpleDb};
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{Deserialize, Serialize};
use foundation_unicode::{NormalizationAccuracy, Normalizer};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
pub struct HandleRecord {
    pub address: String,
    pub created_at: u64,
    pub attest_sig: Vec<u8>,
    pub nonce: u64,
    pub version: u16,
    #[cfg(feature = "pq-crypto")]
    pub pq_pubkey: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum HandleError {
    Duplicate,
    BadSig,
    LowNonce,
    Reserved,
    Storage,
}

impl HandleError {
    pub fn code(&self) -> &'static str {
        match self {
            HandleError::Duplicate => "E_DUP_HANDLE",
            HandleError::BadSig => "E_BAD_SIG",
            HandleError::LowNonce => "E_LOW_NONCE",
            HandleError::Reserved => "E_RESERVED",
            HandleError::Storage => "E_STORAGE",
        }
    }
}

struct NormalizedHandle {
    value: String,
    accuracy: NormalizationAccuracy,
}

impl NormalizedHandle {
    fn value(&self) -> &str {
        &self.value
    }

    fn accuracy(&self) -> NormalizationAccuracy {
        self.accuracy
    }
}

pub struct RegistrationOutcome {
    pub address: String,
    pub normalized_handle: String,
    pub accuracy: NormalizationAccuracy,
}

pub struct HandleRegistry {
    db: SimpleDb,
}

impl HandleRegistry {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open_named(names::IDENTITY_HANDLES, path),
        }
    }

    /// Produce the canonical registration message hash for a handle/pubkey/nonce tuple.
    /// This mirrors the logic used during verification so external signers and tests
    /// can generate signatures without duplicating normalization rules.
    ///
    /// Note: This function does NOT check if the handle is reserved - that check
    /// happens in register_handle(). This allows external signers to create signatures
    /// for any handle, and the reservation check is enforced only during registration.
    pub fn registration_message(
        handle: &str,
        pubkey: &[u8],
        nonce: u64,
    ) -> Result<[u8; 32], HandleError> {
        let normalized = Self::normalize(handle).ok_or(HandleError::Reserved)?;
        let handle_norm = normalized.value.clone();
        let mut h = Hasher::new();
        h.update(b"register:");
        h.update(handle_norm.as_bytes());
        h.update(pubkey);
        h.update(&nonce.to_le_bytes());
        Ok(*h.finalize().as_bytes())
    }

    fn normalize(handle: &str) -> Option<NormalizedHandle> {
        let normalized = Normalizer::default().nfkc(handle);
        let trimmed = normalized.as_str().trim();
        if trimmed.is_empty() {
            return None;
        }
        let value = trimmed.to_lowercase();
        let accuracy = normalized.accuracy();
        #[cfg(feature = "telemetry")]
        {
            let label = [accuracy.as_str()];
            crate::telemetry::IDENTITY_HANDLE_NORMALIZATION_TOTAL
                .ensure_handle_for_label_values(&label)
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
        }
        Some(NormalizedHandle { value, accuracy })
    }

    fn reserved(handle: &str) -> bool {
        handle.starts_with("sys/") || handle.starts_with("admin/")
    }

    fn handle_key(handle: &str) -> String {
        format!("handles/{}", handle)
    }
    fn nonce_key(address: &str) -> String {
        format!("nonces/{}", address)
    }
    fn owner_key(address: &str) -> String {
        format!("owners/{}", address)
    }

    pub fn register_handle(
        &mut self,
        handle: &str,
        pubkey: &[u8],
        #[cfg(feature = "pq-crypto")] pq_pubkey: Option<&[u8]>,
        sig: &[u8],
        nonce: u64,
    ) -> Result<RegistrationOutcome, HandleError> {
        let msg = Self::registration_message(handle, pubkey, nonce)?;
        let normalized = Self::normalize(handle).ok_or(HandleError::Reserved)?;
        if Self::reserved(normalized.value()) {
            return Err(HandleError::Reserved);
        }
        let handle_norm = normalized.value.clone();
        let address = crypto_suite::hex::encode(pubkey);
        // nonce check
        let nonce_key = Self::nonce_key(&address);
        if let Some(raw) = self.db.get(&nonce_key) {
            if let Ok(last) = handle_binary::decode_u64(&raw) {
                if nonce <= last {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::IDENTITY_REPLAYS_BLOCKED_TOTAL.inc();
                    return Err(HandleError::LowNonce);
                }
                if nonce > last + 1 {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::IDENTITY_NONCE_SKIPS_TOTAL.inc();
                }
            }
        }
        // verify
        let vk = VerifyingKey::from_bytes(&crate::to_array_32(pubkey).ok_or(HandleError::BadSig)?)
            .map_err(|_| HandleError::BadSig)?;
        let sig = Signature::from_bytes(&crate::to_array_64(sig).ok_or(HandleError::BadSig)?);
        vk.verify(&msg, &sig)
            .map_err(|_| HandleError::BadSig)?;
        // handle duplication
        let key = Self::handle_key(&handle_norm);
        if let Some(raw) = self.db.get(&key) {
            let rec: HandleRecord = handle_binary::decode_record(&raw).unwrap_or(HandleRecord {
                address: String::new(),
                created_at: 0,
                attest_sig: Vec::new(),
                nonce: 0,
                version: 0,
                #[cfg(feature = "pq-crypto")]
                pq_pubkey: None,
            });
            if rec.address != address {
                #[cfg(feature = "telemetry")]
                crate::telemetry::IDENTITY_REGISTRATIONS_TOTAL
                    .ensure_handle_for_label_values(&[HandleError::Duplicate.code()])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .inc();
                return Err(HandleError::Duplicate);
            }
            if nonce <= rec.nonce {
                #[cfg(feature = "telemetry")]
                crate::telemetry::IDENTITY_REPLAYS_BLOCKED_TOTAL.inc();
                return Err(HandleError::LowNonce);
            }
        }
        let record = HandleRecord {
            address: address.clone(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            attest_sig: sig.to_bytes().to_vec(),
            nonce,
            version: 1,
            #[cfg(feature = "pq-crypto")]
            pq_pubkey: pq_pubkey.map(|p| p.to_vec()),
        };
        let bytes = handle_binary::encode_record(&record);
        self.db.insert(&key, bytes);
        // index owner
        let owner_bytes = handle_binary::encode_string(&handle_norm);
        self.db.insert(&Self::owner_key(&address), owner_bytes);
        // update nonce
        let nonce_bytes = handle_binary::encode_u64(nonce);
        self.db.insert(&nonce_key, nonce_bytes);
        #[cfg(feature = "telemetry")]
        crate::telemetry::IDENTITY_REGISTRATIONS_TOTAL
            .ensure_handle_for_label_values(&["ok"])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .inc();
        Ok(RegistrationOutcome {
            address,
            normalized_handle: handle_norm,
            accuracy: normalized.accuracy(),
        })
    }

    pub fn resolve_handle(&self, handle: &str) -> Option<String> {
        let normalized = Self::normalize(handle)?;
        let key = Self::handle_key(normalized.value());
        self.db
            .get(&key)
            .and_then(|raw| handle_binary::decode_record(&raw).ok())
            .map(|r| r.address)
    }

    pub fn handle_of(&self, address: &str) -> Option<String> {
        let key = Self::owner_key(address);
        self.db
            .get(&key)
            .and_then(|raw| handle_binary::decode_string(&raw).ok())
    }

    #[cfg(feature = "pq-crypto")]
    pub fn pq_key_of(&self, handle: &str) -> Option<Vec<u8>> {
        let normalized = Self::normalize(handle)?;
        let key = Self::handle_key(normalized.value());
        self.db
            .get(&key)
            .and_then(|raw| handle_binary::decode_record(&raw).ok())
            .and_then(|r| r.pq_pubkey)
    }
}
