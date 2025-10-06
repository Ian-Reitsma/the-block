use core::cmp::min;

use crate::mac::{hmac_sha256, SHA256_BLOCK_LEN, SHA256_DIGEST_LEN};

use super::{KeyDerivationError, KeyDeriver};

const HASH_LEN: usize = SHA256_DIGEST_LEN;
const BLOCK_SIZE: usize = SHA256_BLOCK_LEN;

#[derive(Clone, Default)]
pub struct InhouseKeyDeriver {
    salt: Option<Vec<u8>>,
}

impl InhouseKeyDeriver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_salt<S: AsRef<[u8]>>(salt: S) -> Self {
        Self {
            salt: Some(salt.as_ref().to_vec()),
        }
    }
}

impl KeyDeriver for InhouseKeyDeriver {
    fn derive_key(
        &self,
        context: &[u8],
        material: &[u8],
    ) -> Result<[u8; HASH_LEN], KeyDerivationError> {
        let ctx = core::str::from_utf8(context).map_err(|_| KeyDerivationError::InvalidContext)?;
        let mut out = [0u8; HASH_LEN];
        derive_key_material(self.salt.as_deref(), ctx.as_bytes(), material, &mut out);
        Ok(out)
    }
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; HASH_LEN] {
    derive_key_with_info(context.as_bytes(), material)
}

pub fn derive_key_with_info(info: &[u8], material: &[u8]) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    derive_key_material(None, info, material, &mut out);
    out
}

pub fn derive_key_with_salt(salt: &[u8], context: &str, material: &[u8]) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    derive_key_material(Some(salt), context.as_bytes(), material, &mut out);
    out
}

pub fn derive_key_material(salt: Option<&[u8]>, info: &[u8], material: &[u8], output: &mut [u8]) {
    assert!(
        output.len() <= 255 * HASH_LEN,
        "hkdf output length too large"
    );
    let prk = hkdf_extract(salt, material);
    hkdf_expand_into(&prk, info, output);
}

fn hkdf_extract(salt: Option<&[u8]>, ikm: &[u8]) -> [u8; HASH_LEN] {
    let zero_salt = [0u8; BLOCK_SIZE];
    let key = salt.unwrap_or(&zero_salt);
    hmac_sha256(key, ikm)
}

fn hkdf_expand_into(prk: &[u8; HASH_LEN], info: &[u8], output: &mut [u8]) {
    let mut counter = 1u8;
    let mut generated = 0usize;
    let mut prev = [0u8; HASH_LEN];
    let mut prev_len = 0usize;

    while generated < output.len() {
        let mut buffer = Vec::with_capacity(prev_len + info.len() + 1);
        buffer.extend_from_slice(&prev[..prev_len]);
        buffer.extend_from_slice(info);
        buffer.push(counter);
        let block = hmac_sha256(prk, &buffer);
        prev.copy_from_slice(&block);
        prev_len = HASH_LEN;

        let take = min(HASH_LEN, output.len() - generated);
        output[generated..generated + take].copy_from_slice(&block[..take]);
        generated += take;
        counter = counter.wrapping_add(1);
    }
}
