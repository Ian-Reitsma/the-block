use super::{KeyDerivationError, KeyDeriver};
use crate::hashing;

#[derive(Default)]
pub struct InhouseKeyDeriver;

impl KeyDeriver for InhouseKeyDeriver {
    fn derive_key(&self, context: &[u8], material: &[u8]) -> Result<[u8; 32], KeyDerivationError> {
        let context =
            core::str::from_utf8(context).map_err(|_| KeyDerivationError::InvalidContext)?;
        Ok(hashing::inhouse::derive_key(context, material))
    }
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; 32] {
    hashing::inhouse::derive_key(context, material)
}
