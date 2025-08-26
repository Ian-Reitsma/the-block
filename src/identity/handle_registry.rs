use crate::SimpleDb;
use blake3::Hasher;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_normalization::UnicodeNormalization;

#[derive(Serialize, Deserialize, Clone)]
pub struct HandleRecord {
    pub address: String,
    pub created_at: u64,
    pub attest_sig: Vec<u8>,
    pub nonce: u64,
    pub version: u16,
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

pub struct HandleRegistry {
    db: SimpleDb,
}

impl HandleRegistry {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open(path),
        }
    }

    fn normalize(handle: &str) -> Option<String> {
        let h: String = handle.nfc().collect();
        let h = h.trim();
        if h.is_empty() {
            return None;
        }
        Some(h.to_lowercase())
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
        sig: &[u8],
        nonce: u64,
    ) -> Result<String, HandleError> {
        let handle_norm = Self::normalize(handle).ok_or(HandleError::Reserved)?;
        if Self::reserved(&handle_norm) {
            return Err(HandleError::Reserved);
        }
        let address = hex::encode(pubkey);
        // nonce check
        let nonce_key = Self::nonce_key(&address);
        if let Some(raw) = self.db.get(&nonce_key) {
            if let Ok(last) = bincode::deserialize::<u64>(&raw) {
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
        // compute message hash
        let mut h = Hasher::new();
        h.update(b"register:");
        h.update(handle_norm.as_bytes());
        h.update(pubkey);
        h.update(&nonce.to_le_bytes());
        let msg = h.finalize();
        // verify
        let vk = VerifyingKey::from_bytes(&crate::to_array_32(pubkey).ok_or(HandleError::BadSig)?)
            .map_err(|_| HandleError::BadSig)?;
        let sig = Signature::from_bytes(&crate::to_array_64(sig).ok_or(HandleError::BadSig)?);
        vk.verify(msg.as_bytes(), &sig)
            .map_err(|_| HandleError::BadSig)?;
        // handle duplication
        let key = Self::handle_key(&handle_norm);
        if let Some(raw) = self.db.get(&key) {
            let rec: HandleRecord = bincode::deserialize(&raw).unwrap_or(HandleRecord {
                address: String::new(),
                created_at: 0,
                attest_sig: Vec::new(),
                nonce: 0,
                version: 0,
            });
            if rec.address != address {
                #[cfg(feature = "telemetry")]
                crate::telemetry::IDENTITY_REGISTRATIONS_TOTAL
                    .with_label_values(&[HandleError::Duplicate.code()])
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
            attest_sig: sig.to_vec(),
            nonce,
            version: 1,
        };
        let bytes = bincode::serialize(&record).map_err(|_| HandleError::Storage)?;
        self.db.insert(&key, bytes);
        // index owner
        let owner_bytes = bincode::serialize(&handle_norm).map_err(|_| HandleError::Storage)?;
        self.db.insert(&Self::owner_key(&address), owner_bytes);
        // update nonce
        let nonce_bytes = bincode::serialize(&nonce).map_err(|_| HandleError::Storage)?;
        self.db.insert(&nonce_key, nonce_bytes);
        #[cfg(feature = "telemetry")]
        crate::telemetry::IDENTITY_REGISTRATIONS_TOTAL
            .with_label_values(&["ok"])
            .inc();
        Ok(address)
    }

    pub fn resolve_handle(&self, handle: &str) -> Option<String> {
        let handle_norm = Self::normalize(handle)?;
        let key = Self::handle_key(&handle_norm);
        self.db
            .get(&key)
            .and_then(|raw| bincode::deserialize::<HandleRecord>(&raw).ok())
            .map(|r| r.address)
    }

    pub fn handle_of(&self, address: &str) -> Option<String> {
        let key = Self::owner_key(address);
        self.db
            .get(&key)
            .and_then(|raw| bincode::deserialize::<String>(&raw).ok())
    }
}
