use crypto_suite::hashing::blake3;
use sys::random;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("{kind} length mismatch: expected {expected}, got {found}")]
    Length {
        kind: &'static str,
        expected: usize,
        found: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> PublicKey<N> {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != N {
            return Err(Error::Length {
                kind: "public key",
                expected: N,
                found: bytes.len(),
            });
        }
        let mut out = [0u8; N];
        out.copy_from_slice(bytes);
        Ok(Self { bytes: out })
    }

    pub fn as_bytes(&self) -> &[u8; N] {
        &self.bytes
    }

    pub fn to_bytes(self) -> [u8; N] {
        self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretKey<const SK: usize, const PK: usize> {
    secret: [u8; SK],
    public: [u8; PK],
}

impl<const SK: usize, const PK: usize> SecretKey<SK, PK> {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != SK {
            return Err(Error::Length {
                kind: "secret key",
                expected: SK,
                found: bytes.len(),
            });
        }
        let mut secret = [0u8; SK];
        secret.copy_from_slice(bytes);
        let public = derive_public::<PK>(&secret);
        Ok(Self { secret, public })
    }

    pub fn as_bytes(&self) -> &[u8; SK] {
        &self.secret
    }

    pub fn to_bytes(self) -> [u8; SK] {
        self.secret
    }

    pub fn public_key(&self) -> PublicKey<PK> {
        PublicKey { bytes: self.public }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ciphertext<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> Ciphertext<N> {
    pub fn as_bytes(&self) -> &[u8; N] {
        &self.bytes
    }

    pub fn to_bytes(self) -> [u8; N] {
        self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedSecret<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> SharedSecret<N> {
    pub fn as_bytes(&self) -> &[u8; N] {
        &self.bytes
    }

    pub fn to_bytes(self) -> [u8; N] {
        self.bytes
    }
}

pub fn keypair<const PK: usize, const SK: usize>() -> (PublicKey<PK>, SecretKey<SK, PK>) {
    let secret = random_secret::<SK>();
    let public = derive_public::<PK>(&secret);
    (PublicKey { bytes: public }, SecretKey { secret, public })
}

pub fn encapsulate<const CT: usize, const SS: usize, const PK: usize>(
    public: &PublicKey<PK>,
) -> (Ciphertext<CT>, SharedSecret<SS>) {
    let mut entropy = [0u8; 32];
    if random::fill_bytes(&mut entropy).is_err() {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"the-block.pqcrypto_kyber.fallback");
        hasher.update(&(std::process::id()).to_le_bytes());
        hasher.finalize_xof(&mut entropy);
    }

    let ciphertext = derive_ciphertext::<CT, PK>(&public.bytes, &entropy);
    let shared = derive_shared::<SS, PK>(&public.bytes, &entropy);
    (
        Ciphertext { bytes: ciphertext },
        SharedSecret { bytes: shared },
    )
}

fn random_secret<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    if random::fill_bytes(&mut out).is_err() {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"the-block.pqcrypto_kyber.secret");
        hasher.update(&(std::process::id()).to_le_bytes());
        hasher.finalize_xof(&mut out);
    }
    out
}

fn derive_public<const N: usize>(secret: &[u8]) -> [u8; N] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"the-block.kyber.public");
    hasher.update(&(N as u32).to_le_bytes());
    hasher.update(secret);
    let mut out = [0u8; N];
    hasher.finalize_xof(&mut out);
    out
}

fn derive_ciphertext<const CT: usize, const PK: usize>(
    public: &[u8; PK],
    entropy: &[u8; 32],
) -> [u8; CT] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"the-block.kyber.ciphertext");
    hasher.update(&(CT as u32).to_le_bytes());
    hasher.update(entropy);
    hasher.update(&public[..32]);
    let mut out = [0u8; CT];
    hasher.finalize_xof(&mut out);
    out
}

fn derive_shared<const SS: usize, const PK: usize>(
    public: &[u8; PK],
    entropy: &[u8; 32],
) -> [u8; SS] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"the-block.kyber.shared");
    hasher.update(&(SS as u32).to_le_bytes());
    hasher.update(entropy);
    hasher.update(&public[public.len() - 32..]);
    let mut out = [0u8; SS];
    hasher.finalize_xof(&mut out);
    out
}
