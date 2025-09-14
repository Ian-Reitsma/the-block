use crate::{SignedTransaction, TxAdmissionError};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Trait for pluggable account validation logic.
pub trait AccountValidation {
    /// Validate a transaction against account-specific rules.
    fn validate_tx(&mut self, tx: &SignedTransaction) -> Result<(), TxAdmissionError>;
}

/// Policy describing an authorized session key.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct SessionPolicy {
    /// Session public key bytes.
    pub public_key: Vec<u8>,
    /// Expiration time (UNIX secs).
    pub expires_at: u64,
    /// Highest nonce observed for this session.
    #[serde(default)]
    pub nonce: u64,
}

impl SessionPolicy {
    /// Returns true if the policy has expired relative to current system time.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at
    }
}
