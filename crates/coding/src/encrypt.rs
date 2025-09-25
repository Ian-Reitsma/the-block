use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};

use crate::error::{CodingError, EncryptError};

pub const CHACHA20_POLY1305_KEY_LEN: usize = 32;
pub const CHACHA20_POLY1305_NONCE_LEN: usize = 12;
pub const CHACHA20_POLY1305_TAG_LEN: usize = 16;

pub trait Encryptor: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn key_bytes(&self) -> &[u8];
    fn nonce_len(&self) -> usize;
    fn overhead(&self) -> usize;
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptError>;
    fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, EncryptError>;
}

pub fn encryptor_for(name: &str, key: &[u8]) -> Result<Box<dyn Encryptor>, CodingError> {
    match name {
        "" | "chacha20" | "chacha20-poly1305" | "chacha20poly1305" => {
            Ok(Box::new(ChaCha20Poly1305Encryptor::new(key)?))
        }
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_encryptor(key: &[u8]) -> Result<Box<dyn Encryptor>, CodingError> {
    encryptor_for("chacha20-poly1305", key)
}

pub struct ChaCha20Poly1305Encryptor {
    cipher: ChaCha20Poly1305,
    key: Vec<u8>,
}

impl ChaCha20Poly1305Encryptor {
    pub fn new(key: &[u8]) -> Result<Self, EncryptError> {
        if key.len() != CHACHA20_POLY1305_KEY_LEN {
            return Err(EncryptError::InvalidKeyLength {
                expected: CHACHA20_POLY1305_KEY_LEN,
                actual: key.len(),
            });
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        Ok(Self {
            cipher,
            key: key.to_vec(),
        })
    }
}

impl Encryptor for ChaCha20Poly1305Encryptor {
    fn algorithm(&self) -> &'static str {
        "chacha20poly1305"
    }

    fn key_bytes(&self) -> &[u8] {
        &self.key
    }

    fn nonce_len(&self) -> usize {
        CHACHA20_POLY1305_NONCE_LEN
    }

    fn overhead(&self) -> usize {
        CHACHA20_POLY1305_NONCE_LEN + CHACHA20_POLY1305_TAG_LEN
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
        let mut nonce_bytes = [0u8; CHACHA20_POLY1305_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut output = nonce_bytes.to_vec();
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| EncryptError::EncryptionFailed)?;
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, EncryptError> {
        if payload.len() < CHACHA20_POLY1305_NONCE_LEN {
            return Err(EncryptError::InvalidCiphertext { len: payload.len() });
        }
        let (nonce_bytes, ciphertext) = payload.split_at(CHACHA20_POLY1305_NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| EncryptError::DecryptionFailed)
    }
}
