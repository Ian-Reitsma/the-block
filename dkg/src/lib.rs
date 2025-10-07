#![deny(warnings)]

//! First-party distributed key generation primitives.
//!
//! The implementation is intentionally lightweight and deterministic. It does
//! not aim to provide production-grade cryptography; instead it offers a
//! compatible surface for the rest of the workspace while the full in-house
//! threshold scheme is being developed. The types mirror the third-party API
//! that previously powered the `dkg` crate so downstream code continues to
//! compile during the transition.

use std::collections::BTreeMap;

use rand::{thread_rng, RngCore};

/// Represents the group public keys derived from a secret polynomial.
#[derive(Debug, Clone)]
pub struct PublicKeySet {
    threshold: usize,
    seed: u64,
}

impl PublicKeySet {
    /// Returns the aggregate public key corresponding to the secret set.
    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            token: derive_public_token(self.seed),
        }
    }

    /// Combine signature shares into a full signature. Shares are validated by
    /// checking that each contribution matches the message digest when blended
    /// with the internal seed for the corresponding participant.
    pub fn combine_signatures(
        &self,
        shares: &BTreeMap<usize, SignatureShare>,
    ) -> Result<Signature, CombineError> {
        if shares.len() < self.threshold {
            return Err(CombineError::NotEnoughShares);
        }

        let mut digest = None;
        for (id, share) in shares {
            let expected = derive_share_token(self.seed, *id as u64);
            if share.auth != (share.digest ^ expected) {
                return Err(CombineError::InvalidShare);
            }
            digest = match digest {
                None => Some(share.digest),
                Some(existing) => {
                    if existing != share.digest {
                        return Err(CombineError::MismatchedShares);
                    }
                    Some(existing)
                }
            };
        }

        let digest = digest.unwrap_or_default();
        let public_token = derive_public_token(self.seed);
        Ok(Signature {
            digest,
            token: digest ^ public_token,
        })
    }
}

/// Public key used to verify signatures.
#[derive(Debug, Clone)]
pub struct PublicKey {
    token: u64,
}

impl PublicKey {
    /// Verify the provided signature against the message bytes.
    pub fn verify(&self, sig: &Signature, msg: &[u8]) -> bool {
        sig.digest == hash_message(msg) && sig.token == (sig.digest ^ self.token)
    }
}

/// Secret key polynomial used to derive shares for participants.
#[derive(Debug, Clone)]
pub struct SecretKeySet {
    threshold: usize,
    seed: u64,
}

impl SecretKeySet {
    /// Construct a new secret set. The `degree` parameter mirrors the previous
    /// dependency API and is translated into a threshold internally.
    pub fn random<R: RngCore + ?Sized>(degree: usize, rng: &mut R) -> Self {
        let seed = rng.next_u64();
        Self {
            threshold: degree + 1,
            seed,
        }
    }

    /// Return the public keys corresponding to the secret set.
    pub fn public_keys(&self) -> PublicKeySet {
        PublicKeySet {
            threshold: self.threshold,
            seed: self.seed,
        }
    }

    /// Derive the secret share for a specific participant index.
    pub fn secret_key_share(&self, index: usize) -> SecretKeyShare {
        SecretKeyShare {
            id: index as u64,
            seed: self.seed,
        }
    }
}

/// Participant-specific secret share.
#[derive(Debug, Clone)]
pub struct SecretKeyShare {
    id: u64,
    seed: u64,
}

impl SecretKeyShare {
    /// Sign the message bytes, returning a share that can be combined with
    /// others. The share authenticates the participant index using a derived
    /// token so the combiner can reject mismatched or tampered contributions.
    pub fn sign(&self, msg: &[u8]) -> SignatureShare {
        let digest = hash_message(msg);
        let auth = digest ^ derive_share_token(self.seed, self.id);
        SignatureShare {
            participant: self.id,
            digest,
            auth,
        }
    }
}

/// Signature share produced by a participant.
#[derive(Debug, Clone)]
pub struct SignatureShare {
    participant: u64,
    digest: u64,
    auth: u64,
}

impl SignatureShare {
    /// Participant identifier associated with the share.
    pub fn participant(&self) -> u64 {
        self.participant
    }
}

/// Combined signature returned after gathering enough shares.
#[derive(Debug, Clone)]
pub struct Signature {
    digest: u64,
    token: u64,
}

/// Errors surfaced while combining signature shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineError {
    NotEnoughShares,
    InvalidShare,
    MismatchedShares,
}

/// Runs a basic DKG, returning the group public key set and participant secret
/// shares. The current implementation is intentionally simple: it derives a
/// shared seed and hands individual participants deterministic views of that
/// seed so they can produce authenticated signature shares.
pub fn run_dkg(participants: usize, threshold: usize) -> (PublicKeySet, Vec<SecretKeyShare>) {
    let mut rng = thread_rng();
    let sk_set = SecretKeySet::random(threshold.saturating_sub(1), &mut rng);
    let pk_set = sk_set.public_keys();
    let shares = (0..participants)
        .map(|idx| sk_set.secret_key_share(idx))
        .collect();
    (pk_set, shares)
}

/// Combines signature shares into a full signature.
pub fn combine(
    pk: &PublicKeySet,
    msg: &[u8],
    shares: &[(u64, SignatureShare)],
) -> Option<Signature> {
    let mut map = BTreeMap::new();
    for (id, share) in shares {
        map.insert(*id as usize, share.clone());
    }
    let sig = pk.combine_signatures(&map).ok()?;
    if pk.public_key().verify(&sig, msg) {
        Some(sig)
    } else {
        None
    }
}

fn hash_message(msg: &[u8]) -> u64 {
    let mut hash = 0u64;
    for chunk in msg.chunks(8) {
        let mut buf = [0u8; 8];
        let len = chunk.len();
        buf[..len].copy_from_slice(chunk);
        hash ^= u64::from_le_bytes(buf).rotate_left(len as u32 + 1);
        hash = hash.rotate_left(3) ^ 0x9e37_79b9_7f4a_7c15;
    }
    hash
}

fn derive_share_token(seed: u64, participant: u64) -> u64 {
    let mix = seed.rotate_left((participant as u32 % 63) + 1);
    mix ^ (participant.wrapping_mul(0x9e37_79b9))
}

fn derive_public_token(seed: u64) -> u64 {
    seed.rotate_left(17) ^ 0xd6e8_feb8_6659_fd93
}
