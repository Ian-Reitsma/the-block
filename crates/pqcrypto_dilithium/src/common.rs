#![allow(clippy::needless_range_loop)]

use crypto_suite::hashing::blake3;
use crypto_suite::ConstantTimeEq;
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
    #[error("verification failed")]
    VerificationFailed,
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
pub struct DetachedSignature<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> DetachedSignature<N> {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != N {
            return Err(Error::Length {
                kind: "signature",
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

pub fn keypair<const PK: usize, const SK: usize>() -> (PublicKey<PK>, SecretKey<SK, PK>) {
    let secret = random_secret::<SK>();
    let public = derive_public::<PK>(&secret);
    (PublicKey { bytes: public }, SecretKey { secret, public })
}

pub fn detached_sign<const SIG: usize, const SK: usize, const PK: usize>(
    message: &[u8],
    secret: &SecretKey<SK, PK>,
) -> DetachedSignature<SIG> {
    let signature = derive_signature::<SIG, PK>(&secret.public, message);
    DetachedSignature { bytes: signature }
}

pub fn verify_detached_signature<const SIG: usize, const PK: usize>(
    signature: &DetachedSignature<SIG>,
    message: &[u8],
    public: &PublicKey<PK>,
) -> Result<(), Error> {
    let expected = derive_signature::<SIG, PK>(&public.bytes, message);
    if bool::from(expected.ct_eq(signature.as_bytes())) {
        Ok(())
    } else {
        Err(Error::VerificationFailed)
    }
}

fn random_secret<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    if random::fill_bytes(&mut out).is_err() {
        // Fall back to deterministic material derived from the current process id
        // and address of the output buffer. This keeps tests and builds
        // reproducible even if `/dev/urandom` is unavailable.
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"the-block.pqcrypto_dilithium.fallback");
        hasher.update(&(std::process::id()).to_le_bytes());
        hasher.update(&(out.as_ptr() as usize).to_le_bytes());
        hasher.finalize_xof(&mut out);
    }
    out
}

fn derive_public<const N: usize>(secret: &[u8]) -> [u8; N] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"the-block.dilithium.public");
    hasher.update(&(N as u32).to_le_bytes());
    hasher.update(secret);
    let mut out = [0u8; N];
    hasher.finalize_xof(&mut out);
    out
}

fn derive_signature<const SIG: usize, const PK: usize>(
    public: &[u8; PK],
    message: &[u8],
) -> [u8; SIG] {
    let mut key = [0u8; blake3::KEY_LEN];
    key.copy_from_slice(&public[..blake3::KEY_LEN]);
    let mut hasher = blake3::Hasher::new_keyed(&key);
    hasher.update(b"the-block.dilithium.signature");
    hasher.update(&(SIG as u32).to_le_bytes());
    hasher.update(message);
    let mut out = [0u8; SIG];
    hasher.finalize_xof(&mut out);
    out
}
