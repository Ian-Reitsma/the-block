use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

use crate::key_derivation::inhouse;
use crate::mac::hmac_sha256;

use super::symmetric::{decrypt_aes256_cbc, encrypt_aes256_cbc, SymmetricError};
use super::x25519::{KeyError, PublicKey, SecretKey};

const VERSION: u8 = 1;
const RECIPIENT_MAGIC: &[u8; 4] = b"TBXE";
const PASSWORD_MAGIC: &[u8; 4] = b"TBPW";
const SALT_LEN: usize = 16;
const MAC_LEN: usize = 32;
const DERIVED_LEN: usize = 32 + 16 + 32; // key + iv + mac key
const RECIPIENT_INFO: &[u8] = b"tb-export-recipient";
const PASSWORD_INFO: &[u8] = b"tb-export-password";

pub const RECIPIENT_CONTENT_TYPE: &str = "application/tb-envelope";
pub const PASSWORD_CONTENT_TYPE: &str = "application/tb-password-envelope";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    #[error("invalid key: {0}")]
    Key(String),
    #[error("encryption error: {0}")]
    Encrypt(String),
    #[error("decryption error: {0}")]
    Decrypt(String),
    #[error("authentication failure")]
    Authentication,
    #[error("invalid envelope format")]
    InvalidFormat,
}

impl From<KeyError> for EnvelopeError {
    fn from(err: KeyError) -> Self {
        Self::Key(err.to_string())
    }
}

impl From<SymmetricError> for EnvelopeError {
    fn from(err: SymmetricError) -> Self {
        Self::Decrypt(err.to_string())
    }
}

pub fn encrypt_for_recipient(
    plaintext: &[u8],
    recipient: &PublicKey,
) -> Result<Vec<u8>, EnvelopeError> {
    let mut rng = OsRng;
    let ephemeral = SecretKey::generate(&mut rng);
    let shared = ephemeral.diffie_hellman(recipient);

    let mut salt = [0u8; SALT_LEN];
    rng.fill_bytes(&mut salt);

    let (key, iv, mac_key) = derive_material(&shared.to_bytes(), &salt, RECIPIENT_INFO);
    let ciphertext = encrypt_aes256_cbc(&key, &iv, plaintext);

    let mut envelope =
        Vec::with_capacity(RECIPIENT_MAGIC.len() + 1 + 32 + SALT_LEN + ciphertext.len() + MAC_LEN);
    envelope.extend_from_slice(RECIPIENT_MAGIC);
    envelope.push(VERSION);
    envelope.extend_from_slice(&ephemeral.public_key().to_bytes());
    envelope.extend_from_slice(&salt);
    envelope.extend_from_slice(&ciphertext);
    let mac = hmac_sha256(&mac_key, &envelope);
    envelope.extend_from_slice(&mac);
    Ok(envelope)
}

pub fn decrypt_with_secret(
    ciphertext: &[u8],
    secret: &SecretKey,
) -> Result<Vec<u8>, EnvelopeError> {
    if ciphertext.len() < RECIPIENT_MAGIC.len() + 1 + 32 + SALT_LEN + MAC_LEN {
        return Err(EnvelopeError::InvalidFormat);
    }
    if &ciphertext[..RECIPIENT_MAGIC.len()] != RECIPIENT_MAGIC {
        return Err(EnvelopeError::InvalidFormat);
    }
    if ciphertext[RECIPIENT_MAGIC.len()] != VERSION {
        return Err(EnvelopeError::InvalidFormat);
    }
    let offset = RECIPIENT_MAGIC.len() + 1;
    let mut peer_bytes = [0u8; 32];
    peer_bytes.copy_from_slice(&ciphertext[offset..offset + 32]);
    let peer = PublicKey::from_bytes(&peer_bytes)?;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&ciphertext[offset + 32..offset + 32 + SALT_LEN]);
    let mac_start = ciphertext.len() - MAC_LEN;
    let body = &ciphertext[..mac_start];
    let provided_mac = &ciphertext[mac_start..];

    let shared = secret.diffie_hellman(&peer);
    let (key, iv, mac_key) = derive_material(&shared.to_bytes(), &salt, RECIPIENT_INFO);
    let expected_mac = hmac_sha256(&mac_key, body);
    if !constant_time_eq(&expected_mac, provided_mac) {
        return Err(EnvelopeError::Authentication);
    }
    let payload = &ciphertext[offset + 32 + SALT_LEN..mac_start];
    let plain = decrypt_aes256_cbc(&key, &iv, payload)?;
    Ok(plain)
}

pub fn encrypt_with_password(plaintext: &[u8], password: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let mut rng = OsRng;
    let mut salt = [0u8; SALT_LEN];
    rng.fill_bytes(&mut salt);
    let (key, iv, mac_key) = derive_material(password, &salt, PASSWORD_INFO);
    let ciphertext = encrypt_aes256_cbc(&key, &iv, plaintext);
    let mut envelope =
        Vec::with_capacity(PASSWORD_MAGIC.len() + 1 + SALT_LEN + ciphertext.len() + MAC_LEN);
    envelope.extend_from_slice(PASSWORD_MAGIC);
    envelope.push(VERSION);
    envelope.extend_from_slice(&salt);
    envelope.extend_from_slice(&ciphertext);
    let mac = hmac_sha256(&mac_key, &envelope);
    envelope.extend_from_slice(&mac);
    Ok(envelope)
}

pub fn decrypt_with_password(ciphertext: &[u8], password: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    if ciphertext.len() < PASSWORD_MAGIC.len() + 1 + SALT_LEN + MAC_LEN {
        return Err(EnvelopeError::InvalidFormat);
    }
    if &ciphertext[..PASSWORD_MAGIC.len()] != PASSWORD_MAGIC {
        return Err(EnvelopeError::InvalidFormat);
    }
    if ciphertext[PASSWORD_MAGIC.len()] != VERSION {
        return Err(EnvelopeError::InvalidFormat);
    }
    let offset = PASSWORD_MAGIC.len() + 1;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&ciphertext[offset..offset + SALT_LEN]);
    let mac_start = ciphertext.len() - MAC_LEN;
    let body = &ciphertext[..mac_start];
    let provided_mac = &ciphertext[mac_start..];
    let (key, iv, mac_key) = derive_material(password, &salt, PASSWORD_INFO);
    let expected_mac = hmac_sha256(&mac_key, body);
    if !constant_time_eq(&expected_mac, provided_mac) {
        return Err(EnvelopeError::Authentication);
    }
    let payload = &ciphertext[offset + SALT_LEN..mac_start];
    let plain = decrypt_aes256_cbc(&key, &iv, payload)?;
    Ok(plain)
}

fn derive_material(
    material: &[u8],
    salt: &[u8; SALT_LEN],
    info: &[u8],
) -> ([u8; 32], [u8; 16], [u8; 32]) {
    let mut output = [0u8; DERIVED_LEN];
    inhouse::derive_key_material(Some(salt), info, material, &mut output);
    let mut key = [0u8; 32];
    key.copy_from_slice(&output[..32]);
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&output[32..48]);
    let mut mac_key = [0u8; 32];
    mac_key.copy_from_slice(&output[48..]);
    (key, iv, mac_key)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recipient() {
        let mut rng = OsRng;
        let secret = SecretKey::generate(&mut rng);
        let recipient = secret.public_key();
        let plaintext = b"hello world".to_vec();
        let envelope = encrypt_for_recipient(&plaintext, &recipient).unwrap();
        let decrypted = decrypt_with_secret(&envelope, &secret).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn round_trip_password() {
        let plaintext = b"metrics".to_vec();
        let envelope = encrypt_with_password(&plaintext, b"password").unwrap();
        let decrypted = decrypt_with_password(&envelope, b"password").unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn authentication_failure() {
        let mut rng = OsRng;
        let secret = SecretKey::generate(&mut rng);
        let recipient = secret.public_key();
        let mut envelope = encrypt_for_recipient(b"hello", &recipient).unwrap();
        let last = envelope.len() - 1;
        envelope[last] ^= 0xff;
        assert!(matches!(
            decrypt_with_secret(&envelope, &secret),
            Err(EnvelopeError::Authentication)
        ));
    }
}
