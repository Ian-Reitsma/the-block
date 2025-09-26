use hkdf::Hkdf;
use sha2::Sha256;

use super::{KeyDerivationError, KeyDeriver};

#[derive(Default)]
pub struct HkdfSha256;

impl KeyDeriver for HkdfSha256 {
    fn derive_key(&self, context: &[u8], material: &[u8]) -> Result<[u8; 32], KeyDerivationError> {
        let hkdf = Hkdf::<Sha256>::new(None, material);
        let mut okm = [0u8; 32];
        hkdf.expand(context, &mut okm)
            .map_err(|_| KeyDerivationError::DerivationFailed)?;
        Ok(okm)
    }
}

pub fn derive_key(master: &[u8], info: &[u8]) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(None, master);
    let mut okm = [0u8; 32];
    hkdf.expand(info, &mut okm).expect("hkdf expand");
    okm
}
