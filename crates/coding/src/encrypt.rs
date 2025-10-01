mod inhouse;

use crate::error::{CodingError, EncryptError};

pub const CHACHA20_POLY1305_KEY_LEN: usize = inhouse::KEY_LEN;
pub const CHACHA20_POLY1305_NONCE_LEN: usize = inhouse::NONCE_LEN;
pub const CHACHA20_POLY1305_TAG_LEN: usize = inhouse::TAG_LEN;
pub const XCHACHA20_POLY1305_NONCE_LEN: usize = inhouse::XNONCE_LEN;

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

fn key_array(key: &[u8]) -> Result<[u8; CHACHA20_POLY1305_KEY_LEN], EncryptError> {
    if key.len() != CHACHA20_POLY1305_KEY_LEN {
        return Err(EncryptError::InvalidKeyLength {
            expected: CHACHA20_POLY1305_KEY_LEN,
            actual: key.len(),
        });
    }
    let mut buf = [0u8; CHACHA20_POLY1305_KEY_LEN];
    buf.copy_from_slice(key);
    Ok(buf)
}

pub fn encrypt_xchacha20_poly1305(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let key = key_array(key)?;
    inhouse::encrypt_xchacha(&key, plaintext)
}

pub fn encrypt_xchacha20_poly1305_with_nonce(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, EncryptError> {
    let key = key_array(key)?;
    if nonce.len() != XCHACHA20_POLY1305_NONCE_LEN {
        return Err(EncryptError::InvalidCiphertext { len: nonce.len() });
    }
    let mut arr = [0u8; XCHACHA20_POLY1305_NONCE_LEN];
    arr.copy_from_slice(nonce);
    inhouse::encrypt_xchacha_with_nonce(&key, &arr, plaintext)
}

pub fn decrypt_xchacha20_poly1305(key: &[u8], payload: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let key = key_array(key)?;
    inhouse::decrypt_xchacha(&key, payload)
}

pub struct ChaCha20Poly1305Encryptor {
    key: [u8; CHACHA20_POLY1305_KEY_LEN],
}

impl ChaCha20Poly1305Encryptor {
    pub fn new(key: &[u8]) -> Result<Self, EncryptError> {
        if key.len() != CHACHA20_POLY1305_KEY_LEN {
            return Err(EncryptError::InvalidKeyLength {
                expected: CHACHA20_POLY1305_KEY_LEN,
                actual: key.len(),
            });
        }
        let mut buf = [0u8; CHACHA20_POLY1305_KEY_LEN];
        buf.copy_from_slice(key);
        Ok(Self { key: buf })
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
        inhouse::encrypt(&self.key, plaintext)
    }

    fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, EncryptError> {
        inhouse::decrypt(&self.key, payload)
    }
}
