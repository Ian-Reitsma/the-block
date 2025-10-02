use super::{inhouse, KeyDerivationError, KeyDeriver};

#[derive(Clone, Default)]
pub struct HkdfSha256 {
    inner: inhouse::InhouseKeyDeriver,
}

impl HkdfSha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_salt<S: AsRef<[u8]>>(salt: S) -> Self {
        Self {
            inner: inhouse::InhouseKeyDeriver::with_salt(salt),
        }
    }
}

impl KeyDeriver for HkdfSha256 {
    fn derive_key(&self, context: &[u8], material: &[u8]) -> Result<[u8; 32], KeyDerivationError> {
        self.inner.derive_key(context, material)
    }
}

pub fn derive_key(master: &[u8], info: &[u8]) -> [u8; 32] {
    inhouse::derive_key_with_info(info, master)
}
