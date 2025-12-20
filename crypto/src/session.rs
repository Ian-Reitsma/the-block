use crate::primitives::rng::{OsRng, RngError};
use crypto_suite::signatures::ed25519::{SigningKey, SECRET_KEY_LENGTH};

/// Ephemeral session key with expiration timestamp.
#[derive(Clone)]
pub struct SessionKey {
    /// Signing key used for meta-transactions.
    pub secret: SigningKey,
    /// Corresponding public key bytes.
    pub public_key: Vec<u8>,
    /// UNIX timestamp (secs) when the key expires.
    pub expires_at: u64,
}

impl SessionKey {
    /// Generate a new session key expiring at `expires_at` (UNIX secs).
    pub fn generate(expires_at: u64) -> Result<Self, RngError> {
        let mut rng = OsRng;
        let mut secret_bytes = [0u8; SECRET_KEY_LENGTH];
        rng.fill_bytes(&mut secret_bytes)?;
        let secret = SigningKey::from_bytes(&secret_bytes);
        let public_key = secret.verifying_key().to_bytes().to_vec();
        Ok(Self {
            secret,
            public_key,
            expires_at,
        })
    }

    /// Check if the session key has expired given `now` (UNIX secs).
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// Sign arbitrary message bytes with the session secret.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.secret.sign(msg).to_bytes()
    }
}
