//! In-house Ed25519 implementation built on vendored curve25519 arithmetic.

mod field;
mod point;
mod scalar;
mod sha512;

pub use sha512::Sha512;

use core::fmt;
use core::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use rand::{CryptoRng, RngCore};
use thiserror::Error;

use point::{CompressedPoint, EdwardsPoint};
use scalar::Scalar;

pub const PUBLIC_KEY_LENGTH: usize = 32;
pub const SECRET_KEY_LENGTH: usize = 32;
pub const KEYPAIR_LENGTH: usize = SECRET_KEY_LENGTH + PUBLIC_KEY_LENGTH;
pub const SIGNATURE_LENGTH: usize = 64;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SignatureError {
    #[error("invalid signing key bytes")]
    InvalidKey,
    #[error("invalid signature bytes")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("provided public key does not match secret key")]
    KeyMismatch,
}

pub struct SigningKey {
    seed: [u8; SECRET_KEY_LENGTH],
    expanded: Arc<OnceLock<ExpandedSecretKey>>,
    verifying: Arc<OnceLock<VerifyingKey>>,
}

impl Clone for SigningKey {
    fn clone(&self) -> Self {
        Self {
            seed: self.seed,
            expanded: Arc::clone(&self.expanded),
            verifying: Arc::clone(&self.verifying),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    bytes: [u8; SIGNATURE_LENGTH],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyingKey {
    point: EdwardsPoint,
}

impl SigningKey {
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        rng.fill_bytes(&mut seed);
        Self {
            seed,
            expanded: Arc::new(OnceLock::new()),
            verifying: Arc::new(OnceLock::new()),
        }
    }

    pub fn from_bytes(bytes: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self {
            seed: *bytes,
            expanded: Arc::new(OnceLock::new()),
            verifying: Arc::new(OnceLock::new()),
        }
    }

    pub fn from_keypair_bytes(bytes: &[u8; KEYPAIR_LENGTH]) -> Result<Self, SignatureError> {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        seed.copy_from_slice(&bytes[..SECRET_KEY_LENGTH]);
        let mut provided_public = [0u8; PUBLIC_KEY_LENGTH];
        provided_public.copy_from_slice(&bytes[SECRET_KEY_LENGTH..]);

        let signing = Self {
            seed,
            expanded: Arc::new(OnceLock::new()),
            verifying: Arc::new(OnceLock::new()),
        };
        if signing.verifying_key().to_bytes() != provided_public {
            return Err(SignatureError::KeyMismatch);
        }

        Ok(signing)
    }

    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.seed
    }

    pub fn to_keypair_bytes(&self) -> [u8; KEYPAIR_LENGTH] {
        let mut out = [0u8; KEYPAIR_LENGTH];
        out[..SECRET_KEY_LENGTH].copy_from_slice(&self.seed);
        out[SECRET_KEY_LENGTH..].copy_from_slice(&self.verifying_key().to_bytes());
        out
    }

    fn expanded(&self) -> &ExpandedSecretKey {
        self.expanded
            .get_or_init(|| ExpandedSecretKey::from_seed(&self.seed))
    }

    fn verifying_cached(&self) -> &VerifyingKey {
        self.verifying.get_or_init(|| {
            let expanded = self.expanded();
            VerifyingKey {
                point: EdwardsPoint::mul_base(&expanded.scalar),
            }
        })
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_cached().clone()
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        let expanded = self.expanded();
        let public = self.verifying_cached();

        let r_digest = Sha512::digest_chunks(&[&expanded.prefix, message]);
        let r_scalar = Scalar::from_bytes_mod_order_wide(&r_digest);

        let r_point = EdwardsPoint::mul_base(&r_scalar);
        let r_encoded = r_point.compress();

        let public_bytes = public.to_bytes();
        let k_digest = Sha512::digest_chunks(&[&r_encoded, &public_bytes, message]);
        let k_scalar = Scalar::from_bytes_mod_order_wide(&k_digest);

        let s = Scalar::mul_add(&k_scalar, &expanded.scalar, &r_scalar);

        let mut bytes = [0u8; SIGNATURE_LENGTH];
        bytes[..32].copy_from_slice(&r_encoded);
        bytes[32..].copy_from_slice(&s.to_bytes());

        Signature { bytes }
    }
}

impl crate::signatures::Signer for SigningKey {
    type Signature = Signature;

    fn sign(&self, message: &[u8]) -> Self::Signature {
        self.sign(message)
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey").finish_non_exhaustive()
    }
}

impl Signature {
    pub fn from_bytes(bytes: &[u8; SIGNATURE_LENGTH]) -> Self {
        Self { bytes: *bytes }
    }

    pub fn to_bytes(self) -> [u8; SIGNATURE_LENGTH] {
        self.bytes
    }
}

impl From<Signature> for [u8; SIGNATURE_LENGTH] {
    fn from(value: Signature) -> Self {
        value.bytes
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature")
            .field(&crate::hex::encode(self.bytes))
            .finish()
    }
}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.bytes);
    }
}

impl VerifyingKey {
    pub fn from_bytes(bytes: &[u8; PUBLIC_KEY_LENGTH]) -> Result<Self, SignatureError> {
        let compressed = CompressedPoint(*bytes);
        let point = compressed.decompress().ok_or(SignatureError::InvalidKey)?;
        if point.is_small_order() {
            return Err(SignatureError::InvalidKey);
        }
        Ok(Self { point })
    }

    pub fn to_bytes(&self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.point.compress()
    }

    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), SignatureError> {
        self.verify_internal(message, signature, false)
    }

    pub fn verify_strict(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), SignatureError> {
        self.verify_internal(message, signature, true)
    }

    fn verify_internal(
        &self,
        message: &[u8],
        signature: &Signature,
        strict: bool,
    ) -> Result<(), SignatureError> {
        let mut r_bytes = [0u8; 32];
        let mut s_bytes = [0u8; 32];
        r_bytes.copy_from_slice(&signature.bytes[..32]);
        s_bytes.copy_from_slice(&signature.bytes[32..]);

        if strict && !Scalar::is_canonical(&s_bytes) {
            return Err(SignatureError::InvalidSignature);
        }

        let r_point = CompressedPoint(r_bytes)
            .decompress()
            .ok_or(SignatureError::InvalidSignature)?;
        if r_point.is_small_order() {
            return Err(SignatureError::InvalidSignature);
        }

        let s_scalar =
            Scalar::from_canonical_bytes(&s_bytes).ok_or(SignatureError::InvalidSignature)?;

        let public_bytes = self.to_bytes();
        let k_digest = Sha512::digest_chunks(&[&r_bytes, &public_bytes, message]);
        let k_scalar = Scalar::from_bytes_mod_order_wide(&k_digest);

        let sb = EdwardsPoint::mul_base(&s_scalar);
        let ka = self.point.scalar_mul(&k_scalar);
        let r_plus = ka.add(&r_point);

        if sb.compress() == r_plus.compress() {
            Ok(())
        } else {
            Err(SignatureError::VerificationFailed)
        }
    }
}

impl Hash for VerifyingKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.to_bytes());
    }
}

impl crate::signatures::Verifier<Signature> for VerifyingKey {
    type Error = SignatureError;

    fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), Self::Error> {
        VerifyingKey::verify(self, message, signature)
    }
}

#[derive(Clone)]
struct ExpandedSecretKey {
    scalar: Scalar,
    prefix: [u8; 32],
}

impl ExpandedSecretKey {
    fn from_seed(seed: &[u8; 32]) -> Self {
        let digest = Sha512::digest(seed);
        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(&digest[..32]);
        clamp_scalar(&mut scalar_bytes);
        let mut prefix = [0u8; 32];
        prefix.copy_from_slice(&digest[32..]);
        let scalar = Scalar::from_bytes_mod_order(&scalar_bytes);
        Self { scalar, prefix }
    }
}

fn clamp_scalar(bytes: &mut [u8; 32]) {
    bytes[0] &= 248;
    bytes[31] &= 63;
    bytes[31] |= 64;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_to_array<const N: usize>(hex_str: &str) -> [u8; N] {
        let bytes = crate::hex::decode(hex_str).expect("hex");
        let mut arr = [0u8; N];
        arr.copy_from_slice(&bytes);
        arr
    }

    #[test]
    fn expanded_secret_scalar_matches_rfc_vector() {
        let seed =
            hex_to_array::<32>("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let expanded = ExpandedSecretKey::from_seed(&seed);
        let expected =
            hex_to_array::<32>("7c2cac12e69be96ae9065065462385e8fcff2768d980c0a3a520f006904de90f");
        assert_eq!(expanded.scalar.to_bytes(), expected);
    }

    #[test]
    fn verifying_key_matches_rfc_vector() {
        let seed =
            hex_to_array::<32>("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let signing_key = SigningKey::from_bytes(&seed);
        let expected =
            hex_to_array::<32>("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        assert_eq!(signing_key.verifying_key().to_bytes(), expected);
    }

    #[test]
    fn base_point_multiplication_by_one_matches_generator() {
        let mut bytes = [0u8; 32];
        bytes[0] = 1;
        let scalar = Scalar::from_bytes_mod_order(&bytes);
        let point = EdwardsPoint::mul_base(&scalar);
        let expected =
            hex_to_array::<32>("5866666666666666666666666666666666666666666666666666666666666666");
        assert_eq!(point.compress(), expected);
    }

    fn assert_on_curve(point: &EdwardsPoint) {
        let x2 = point.x.square();
        let y2 = point.y.square();
        let lhs = y2.sub(&x2);
        let dx2y2 = super::field::curve_constant_d().mul(&x2).mul(&y2);
        let rhs = super::field::FieldElement::one().add(&dx2y2);
        assert_eq!(lhs.to_bytes(), rhs.to_bytes());
    }

    #[test]
    fn base_point_lies_on_curve() {
        let (bx, by) = super::field::base_point();
        let base = EdwardsPoint { x: bx, y: by };
        assert_on_curve(&base);
    }

    #[test]
    fn verifying_key_from_seed_lies_on_curve() {
        let seed =
            hex_to_array::<32>("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let signing_key = SigningKey::from_bytes(&seed);
        let vk = signing_key.verifying_key();
        assert_on_curve(&vk.point);
    }

    #[test]
    fn basepoint_add_matches_scalar_addition() {
        // Deterministic scalars to exercise group law: B*(a+b) == B*a + B*b
        let a_bytes =
            hex_to_array::<32>("0100000000000000000000000000000000000000000000000000000000000000");
        let b_bytes =
            hex_to_array::<32>("0200000000000000000000000000000000000000000000000000000000000000");
        let ab_bytes =
            hex_to_array::<32>("0300000000000000000000000000000000000000000000000000000000000000");
        let a = Scalar::from_bytes_mod_order(&a_bytes);
        let b = Scalar::from_bytes_mod_order(&b_bytes);
        let ab = Scalar::from_bytes_mod_order(&ab_bytes);

        let ba = EdwardsPoint::mul_base(&a);
        let bb = EdwardsPoint::mul_base(&b);
        let sum = ba.add(&bb);
        let direct = EdwardsPoint::mul_base(&ab);

        assert_eq!(sum.compress(), direct.compress());
    }

    #[test]
    fn sign_verify_regression_acc7_message() {
        // Regression reproducer from orphan_fuzz flake: sign + verify must succeed.
        let sk =
            hex_to_array::<32>("a0d388c95df8af6e83084a79219e17069346fc0364be38d1655bcba5a68a0fbf");
        let msg = crate::hex::decode("5448455f424c4f434b76327c010000000800000000000000050000000000000066726f6d5f0400000000000000616363370200000000000000746f040000000000000073696e6b0f00000000000000616d6f756e745f636f6e73756d657201000000000000001100000000000000616d6f756e745f696e647573747269616c00000000000000000300000000000000666565e80300000000000003000000000000007063746405000000000000006e6f6e6365010000000000000004000000000000006d656d6f0000000000000000").expect("msg hex");
        let expected_sig = "bda31c8496fafa510788d8f4a0042353cec9a32148493b3074b8f32aadb66e6a2ae40b8fe666e679f90f1b8ac8eb8259d5e4c99ca2a82fa3a7930d2acdee9603";

        let signing_key = SigningKey::from_bytes(&sk);
        let sig = signing_key.sign(&msg);
        assert_eq!(
            crate::hex::encode(sig.to_bytes()),
            expected_sig,
            "signature must be deterministic for regression payload"
        );
        let expanded = signing_key.expanded();
        let a_scalar = expanded.scalar.clone();
        let r_digest = Sha512::digest_chunks(&[&expanded.prefix, &msg]);
        let r_scalar = Scalar::from_bytes_mod_order_wide(&r_digest);
        let vk = signing_key.verifying_key();
        let a_point = EdwardsPoint::mul_base(&a_scalar);
        assert_eq!(
            a_point.compress(),
            vk.point.compress(),
            "public key mismatch"
        );
        let mut r_bytes = [0u8; 32];
        let mut s_bytes = [0u8; 32];
        let sig_bytes = sig.to_bytes();
        r_bytes.copy_from_slice(&sig_bytes[..32]);
        s_bytes.copy_from_slice(&sig_bytes[32..]);
        let r_point = CompressedPoint(r_bytes).decompress().expect("r decompress");
        let s_scalar = Scalar::from_canonical_bytes(&s_bytes).expect("s canonical");
        let public_bytes = vk.to_bytes();
        let k_digest = Sha512::digest_chunks(&[&r_bytes, &public_bytes, &msg]);
        let k_scalar = Scalar::from_bytes_mod_order_wide(&k_digest);
        let sb = EdwardsPoint::mul_base(&s_scalar);
        let ka = vk.point.scalar_mul(&k_scalar);
        let r_plus = ka.add(&r_point);
        let calc_s = Scalar::mul_add(&k_scalar, &a_scalar, &r_scalar);
        let direct = EdwardsPoint::mul_base(&calc_s);
        let recomposed = EdwardsPoint::mul_base(&r_scalar)
            .add(&EdwardsPoint::mul_base(&a_scalar).scalar_mul(&k_scalar));
        let zero = Scalar::from_bytes_mod_order(&[0u8; 32]);
        let ka_scalar = Scalar::mul_add(&k_scalar, &a_scalar, &zero);
        let base_ka = EdwardsPoint::mul_base(&ka_scalar);
        let point_ka = vk.point.scalar_mul(&k_scalar);
        assert_eq!(sb.compress(), r_plus.compress(), "group equation mismatch");
        assert_eq!(
            direct.compress(),
            recomposed.compress(),
            "recomposition mismatch"
        );
        assert_eq!(
            base_ka.compress(),
            point_ka.compress(),
            "scalar multiplication mismatch"
        );
        vk.verify(&msg, &sig)
            .expect("regression signature must verify");
    }
}
