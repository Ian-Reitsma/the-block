use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey, SECRET_KEY_LENGTH};
use crypto_suite::signatures::internal::Sha512;
use rand::rngs::OsRng;
use rand::RngCore;
use std::fmt;

const MASTER_KEY: &[u8] = b"ed25519 seed";
const HARDENED_OFFSET: u32 = 0x8000_0000;
const HMAC_BLOCK_SIZE: usize = 128;

/// Generate a pseudo master seed for HD wallets.
pub fn generate_master() -> [u8; SECRET_KEY_LENGTH] {
    let mut seed = [0u8; SECRET_KEY_LENGTH];
    OsRng::default().fill_bytes(&mut seed);
    seed
}

/// Derived signing/verifying key material for a pseudo HD path.
#[derive(Clone, Debug)]
pub struct Keypair {
    pub secret: SigningKey,
    pub public: VerifyingKey,
}

/// Errors surfaced while parsing HD paths or deriving hardened children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdError {
    EmptyPath,
    InvalidPath,
    InvalidSegment,
    NonHardenedSegment,
}

impl fmt::Display for HdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use HdError::*;
        let description = match self {
            EmptyPath => "HD path was empty",
            InvalidPath => "HD path must start with 'm' and contain slash-separated segments",
            InvalidSegment => "path segment is not a valid index",
            NonHardenedSegment => "only hardened slots (index') are supported for ed25519",
        };
        write!(f, "{description}")
    }
}

impl std::error::Error for HdError {}

/// Derive a child keypair from a master seed and a hardended-only BIP32 path.
pub fn derive_child(master: &[u8; SECRET_KEY_LENGTH], path: &str) -> Result<Keypair, HdError> {
    ExtendedKey::from_seed(master).derive_path(path)?.try_into()
}

struct ExtendedKey {
    secret: SigningKey,
    public: VerifyingKey,
    chain_code: [u8; 32],
}

impl ExtendedKey {
    fn from_seed(seed: &[u8]) -> Self {
        let digest = hmac_sha512(MASTER_KEY, seed);
        let (secret_bytes, chain_code) = split_digest(&digest);
        ExtendedKey::from_components(secret_bytes, chain_code)
    }

    fn from_components(secret_bytes: [u8; SECRET_KEY_LENGTH], chain_code: [u8; 32]) -> Self {
        let secret = SigningKey::from_bytes(&secret_bytes);
        let public = secret.verifying_key();
        Self {
            secret,
            public,
            chain_code,
        }
    }

    fn derive_path(mut self, path: &str) -> Result<Self, HdError> {
        for index in parse_path(path)? {
            self = self.derive_hardened(index)?;
        }
        Ok(self)
    }

    fn derive_hardened(&self, index: u32) -> Result<Self, HdError> {
        if index & HARDENED_OFFSET == 0 {
            return Err(HdError::NonHardenedSegment);
        }
        let mut data = Vec::with_capacity(1 + SECRET_KEY_LENGTH + 4);
        data.push(0);
        data.extend_from_slice(&self.secret.to_bytes());
        data.extend_from_slice(&index.to_be_bytes());
        let digest = hmac_sha512(&self.chain_code, &data);
        let (secret_bytes, chain_code) = split_digest(&digest);
        Ok(ExtendedKey::from_components(secret_bytes, chain_code))
    }

    fn to_keypair(&self) -> Keypair {
        Keypair {
            secret: self.secret.clone(),
            public: self.public.clone(),
        }
    }
}

impl TryFrom<ExtendedKey> for Keypair {
    type Error = HdError;

    fn try_from(value: ExtendedKey) -> Result<Self, Self::Error> {
        Ok(value.to_keypair())
    }
}

fn parse_path(path: &str) -> Result<Vec<u32>, HdError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(HdError::EmptyPath);
    }
    let mut segments = trimmed.split('/');
    let first = segments.next().unwrap_or_default();
    if !first.is_empty() && !matches!(first, "m" | "M") {
        return Err(HdError::InvalidPath);
    }
    let mut indexes = Vec::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let hardened = segment.ends_with('\'');
        let digits = if hardened {
            &segment[..segment.len() - 1]
        } else {
            segment
        };
        if digits.is_empty() {
            return Err(HdError::InvalidSegment);
        }
        if !hardened {
            return Err(HdError::NonHardenedSegment);
        }
        let index = digits.parse::<u32>().map_err(|_| HdError::InvalidSegment)?;
        indexes.push(index | HARDENED_OFFSET);
    }
    Ok(indexes)
}

fn split_digest(digest: &[u8; 64]) -> ([u8; SECRET_KEY_LENGTH], [u8; 32]) {
    let mut secret = [0u8; SECRET_KEY_LENGTH];
    secret.copy_from_slice(&digest[..SECRET_KEY_LENGTH]);
    let mut chain_code = [0u8; 32];
    chain_code.copy_from_slice(&digest[SECRET_KEY_LENGTH..]);
    (secret, chain_code)
}

fn hmac_sha512(key: &[u8], data: &[u8]) -> [u8; 64] {
    let mut key_block = [0u8; HMAC_BLOCK_SIZE];
    if key.len() > HMAC_BLOCK_SIZE {
        let hashed = Sha512::digest(key);
        key_block[..64].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = [0u8; HMAC_BLOCK_SIZE];
    let mut outer = [0u8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        inner[i] = key_block[i] ^ 0x36;
        outer[i] = key_block[i] ^ 0x5c;
    }
    let inner_hash = Sha512::digest_chunks(&[&inner, data]);
    Sha512::digest_chunks(&[&outer, &inner_hash])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_consistent_path() {
        let seed = [0xABu8; SECRET_KEY_LENGTH];
        let kp1 = derive_child(&seed, "m/0'/1'").expect("hardened path works");
        let kp2 = derive_child(&seed, "m/0'/1'").expect("same path matches");
        assert_eq!(kp1.secret.to_bytes(), kp2.secret.to_bytes());
        assert_eq!(kp1.public.to_bytes(), kp2.public.to_bytes());
    }

    #[test]
    fn non_hardened_path_rejected() {
        let seed = [0u8; SECRET_KEY_LENGTH];
        let err = derive_child(&seed, "m/0/1'").unwrap_err();
        assert_eq!(err, HdError::NonHardenedSegment);
    }

    #[test]
    fn parse_path_even_without_prefix() {
        assert_eq!(parse_path("m").unwrap(), Vec::<u32>::new());
        assert!(matches!(
            parse_path("m/0"),
            Err(HdError::NonHardenedSegment)
        ));
    }

    #[test]
    fn hmac_consistency_vector() {
        let digest = hmac_sha512(b"key", b"data");
        assert_eq!(
            hex_to_bytes("3c5953a18f7303ec653ba170ae334fafa08e3846f2efe317b87efce82376253cb52a8c31ddcde5a3a2eee183c2b34cb91f85e64ddbc325f7692b199473579c58"),
            digest
        );
    }

    fn hex_to_bytes(src: &str) -> Vec<u8> {
        assert!(src.len() % 2 == 0);
        src.as_bytes()
            .chunks(2)
            .map(|chunk| {
                u8::from_str_radix(std::str::from_utf8(chunk).expect("valid hex"), 16)
                    .expect("hex decoding")
            })
            .collect()
    }
}
