use core::fmt;
use core::str::FromStr;

use rand::{CryptoRng, RngCore};
use thiserror::Error;

const SCALAR_LEN: usize = 32;
const POINT_LEN: usize = 32;
const BASEPOINT: [u8; POINT_LEN] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const A24: u32 = 121_665;
const MASK_51: i128 = (1 << 51) - 1;
const MODULUS_LIMB0: i128 = MASK_51 - 18;
const MODULUS_LIMB: i128 = MASK_51;
const RECIPIENT_PREFIX: &str = "tbx1";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum KeyError {
    #[error("invalid length: expected {0} bytes")]
    Length(usize),
    #[error("invalid recipient string")]
    InvalidEncoding,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PublicKey {
    bytes: [u8; POINT_LEN],
}

#[derive(Clone, Debug)]
pub struct SecretKey {
    bytes: [u8; SCALAR_LEN],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedSecret {
    bytes: [u8; SCALAR_LEN],
}

impl SecretKey {
    pub fn generate<R>(rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        let mut bytes = [0u8; SCALAR_LEN];
        rng.fill_bytes(&mut bytes);
        clamp_scalar(&mut bytes);
        Self { bytes }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        if bytes.len() != SCALAR_LEN {
            return Err(KeyError::Length(SCALAR_LEN));
        }
        let mut out = [0u8; SCALAR_LEN];
        out.copy_from_slice(bytes);
        clamp_scalar(&mut out);
        Ok(Self { bytes: out })
    }

    pub fn to_bytes(&self) -> [u8; SCALAR_LEN] {
        self.bytes
    }

    pub fn public_key(&self) -> PublicKey {
        let point = scalar_mult(&self.bytes, &BASEPOINT);
        PublicKey { bytes: point }
    }

    pub fn diffie_hellman(&self, peer: &PublicKey) -> SharedSecret {
        let point = scalar_mult(&self.bytes, &peer.bytes);
        SharedSecret { bytes: point }
    }
}

impl PublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        if bytes.len() != POINT_LEN {
            return Err(KeyError::Length(POINT_LEN));
        }
        let mut out = [0u8; POINT_LEN];
        out.copy_from_slice(bytes);
        Ok(Self { bytes: out })
    }

    pub fn to_bytes(&self) -> [u8; POINT_LEN] {
        self.bytes
    }
}

impl SharedSecret {
    pub fn to_bytes(&self) -> [u8; SCALAR_LEN] {
        self.bytes
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", RECIPIENT_PREFIX, hex::encode(self.bytes))
    }
}

impl FromStr for PublicKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(RECIPIENT_PREFIX) {
            return Err(KeyError::InvalidEncoding);
        }
        let rest = &s[RECIPIENT_PREFIX.len()..];
        let bytes = hex::decode(rest).map_err(|_| KeyError::InvalidEncoding)?;
        Self::from_bytes(&bytes)
    }
}

fn clamp_scalar(bytes: &mut [u8; SCALAR_LEN]) {
    bytes[0] &= 248;
    bytes[31] &= 127;
    bytes[31] |= 64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FieldElement {
    limbs: [i128; 5],
}

impl FieldElement {
    fn zero() -> Self {
        Self { limbs: [0; 5] }
    }

    fn one() -> Self {
        let mut limbs = [0i128; 5];
        limbs[0] = 1;
        Self { limbs }
    }

    fn from_u32(value: u32) -> Self {
        let mut limbs = [0i128; 5];
        limbs[0] = value as i128;
        Self { limbs }
    }

    fn from_bytes(bytes: &[u8; POINT_LEN]) -> Self {
        let mut limbs = [0i128; 5];
        let t0 = load_u64(&bytes[0..8]);
        let t1 = load_u64(&bytes[8..16]);
        let t2 = load_u64(&bytes[16..24]);
        let t3 = load_u64(&bytes[24..32]);

        limbs[0] = (t0 as i128) & MASK_51;
        limbs[1] = (((t0 >> 51) as u128 | ((t1 as u128) << 13)) & MASK_51 as u128) as i128;
        limbs[2] = (((t1 >> 38) as u128 | ((t2 as u128) << 26)) & MASK_51 as u128) as i128;
        limbs[3] = (((t2 >> 25) as u128 | ((t3 as u128) << 39)) & MASK_51 as u128) as i128;
        limbs[4] = ((t3 >> 12) as i128) & MASK_51;

        Self { limbs }.reduce()
    }

    fn to_bytes(&self) -> [u8; POINT_LEN] {
        let mut limbs = self.normalize();

        // Final carry propagation to ensure each limb fits into 51 bits
        FieldElement::reduce_limbs(&mut limbs);

        let h0 = limbs[0] as u64;
        let h1 = limbs[1] as u64;
        let h2 = limbs[2] as u64;
        let h3 = limbs[3] as u64;
        let h4 = limbs[4] as u64;

        let mut out = [0u8; POINT_LEN];
        let t0 = (h0 as u128) | ((h1 as u128) << 51);
        let t1 = ((h1 as u128) >> 13) | ((h2 as u128) << 38);
        let t2 = ((h2 as u128) >> 26) | ((h3 as u128) << 25);
        let t3 = ((h3 as u128) >> 39) | ((h4 as u128) << 12);

        out[0..8].copy_from_slice(&((t0 & u64::MAX as u128) as u64).to_le_bytes());
        out[8..16].copy_from_slice(&((t1 & u64::MAX as u128) as u64).to_le_bytes());
        out[16..24].copy_from_slice(&((t2 & u64::MAX as u128) as u64).to_le_bytes());
        out[24..32].copy_from_slice(&((t3 & u64::MAX as u128) as u64).to_le_bytes());

        out
    }

    fn add(&self, other: &Self) -> Self {
        let mut limbs = [0i128; 5];
        for i in 0..5 {
            limbs[i] = self.limbs[i] + other.limbs[i];
        }
        Self { limbs }.reduce()
    }

    fn sub(&self, other: &Self) -> Self {
        let modulus = [
            MODULUS_LIMB0,
            MODULUS_LIMB,
            MODULUS_LIMB,
            MODULUS_LIMB,
            MODULUS_LIMB,
        ];
        let mut limbs = [0i128; 5];
        for i in 0..5 {
            limbs[i] = self.limbs[i] - other.limbs[i] + modulus[i] * 2;
        }
        Self { limbs }.reduce()
    }

    fn mul(&self, other: &Self) -> Self {
        let a0 = self.limbs[0];
        let a1 = self.limbs[1];
        let a2 = self.limbs[2];
        let a3 = self.limbs[3];
        let a4 = self.limbs[4];

        let b0 = other.limbs[0];
        let b1 = other.limbs[1];
        let b2 = other.limbs[2];
        let b3 = other.limbs[3];
        let b4 = other.limbs[4];

        let b1_19 = b1 * 19;
        let b2_19 = b2 * 19;
        let b3_19 = b3 * 19;
        let b4_19 = b4 * 19;

        let mut limbs = [0i128; 5];
        limbs[0] = a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19;
        limbs[1] = a0 * b1 + a1 * b0 + a2 * b4_19 + a3 * b3_19 + a4 * b2_19;
        limbs[2] = a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4_19 + a4 * b3_19;
        limbs[3] = a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4_19;
        limbs[4] = a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0;

        Self { limbs }.reduce()
    }

    fn square(&self) -> Self {
        self.mul(self)
    }

    fn square_n(&self, n: usize) -> Self {
        let mut result = *self;
        for _ in 0..n {
            result = result.square();
        }
        result
    }

    fn invert(&self) -> Self {
        let z2 = self.square();
        let z4 = z2.square();
        let z8 = z4.square();
        let z9 = z8.mul(self);
        let z11 = z9.mul(&z2);
        let z22 = z11.square();
        let z_2_5_0 = z22.mul(&z9);

        let z_2_10_0 = z_2_5_0.square_n(5).mul(&z_2_5_0);
        let z_2_20_0 = z_2_10_0.square_n(10).mul(&z_2_10_0);
        let z_2_40_0 = z_2_20_0.square_n(20).mul(&z_2_20_0);
        let z_2_50_0 = z_2_40_0.square_n(10).mul(&z_2_10_0);
        let z_2_100_0 = z_2_50_0.square_n(50).mul(&z_2_50_0);
        let z_2_200_0 = z_2_100_0.square_n(100).mul(&z_2_100_0);
        let z_2_250_0 = z_2_200_0.square_n(50).mul(&z_2_50_0);
        let z_2_255_21 = z_2_250_0.square_n(5).mul(&z11);

        z_2_255_21
    }

    fn reduce(mut self) -> Self {
        FieldElement::reduce_limbs(&mut self.limbs);
        Self { limbs: self.limbs }
    }

    fn reduce_limbs(limbs: &mut [i128; 5]) {
        let mut carry0 = limbs[0] >> 51;
        limbs[0] -= carry0 << 51;
        limbs[1] += carry0;

        let mut carry1 = limbs[1] >> 51;
        limbs[1] -= carry1 << 51;
        limbs[2] += carry1;

        let mut carry2 = limbs[2] >> 51;
        limbs[2] -= carry2 << 51;
        limbs[3] += carry2;

        let mut carry3 = limbs[3] >> 51;
        limbs[3] -= carry3 << 51;
        limbs[4] += carry3;

        let mut carry4 = limbs[4] >> 51;
        limbs[4] -= carry4 << 51;
        limbs[0] += carry4 * 19;

        carry0 = limbs[0] >> 51;
        limbs[0] -= carry0 << 51;
        limbs[1] += carry0;

        carry1 = limbs[1] >> 51;
        limbs[1] -= carry1 << 51;
        limbs[2] += carry1;

        carry2 = limbs[2] >> 51;
        limbs[2] -= carry2 << 51;
        limbs[3] += carry2;

        carry3 = limbs[3] >> 51;
        limbs[3] -= carry3 << 51;
        limbs[4] += carry3;

        carry4 = limbs[4] >> 51;
        limbs[4] -= carry4 << 51;
        limbs[0] += carry4 * 19;
    }

    fn normalize(&self) -> [i128; 5] {
        let mut limbs = self.limbs;
        FieldElement::reduce_limbs(&mut limbs);

        let modulus = [
            MODULUS_LIMB0,
            MODULUS_LIMB,
            MODULUS_LIMB,
            MODULUS_LIMB,
            MODULUS_LIMB,
        ];

        let mut should_subtract = false;
        for i in (0..5).rev() {
            if limbs[i] > modulus[i] {
                should_subtract = true;
                break;
            } else if limbs[i] < modulus[i] {
                should_subtract = false;
                break;
            }
        }

        if should_subtract {
            let mut borrow = 0i128;
            for i in 0..5 {
                let mut value = limbs[i] - modulus[i] - borrow;
                if value < 0 {
                    value += 1i128 << 51;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                limbs[i] = value;
            }
        }

        limbs
    }
}

fn conditional_swap(a: &mut FieldElement, b: &mut FieldElement, swap: u8) {
    let mask = -((swap & 1) as i128);
    for i in 0..5 {
        let diff = mask & (a.limbs[i] ^ b.limbs[i]);
        a.limbs[i] ^= diff;
        b.limbs[i] ^= diff;
    }
}

fn load_u64(input: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[0..8]);
    u64::from_le_bytes(bytes)
}

fn scalar_mult(scalar: &[u8; SCALAR_LEN], point: &[u8; POINT_LEN]) -> [u8; POINT_LEN] {
    let mut scalar = *scalar;
    clamp_scalar(&mut scalar);

    let x1 = FieldElement::from_bytes(point);
    let mut x2 = FieldElement::one();
    let mut z2 = FieldElement::zero();
    let mut x3 = x1;
    let mut z3 = FieldElement::one();
    let mut swap = 0u8;
    let a24 = FieldElement::from_u32(A24);

    for t in (0..255).rev() {
        let k_t = (scalar[t / 8] >> (t & 7)) & 1;
        let toggle = swap ^ k_t;
        conditional_swap(&mut x2, &mut x3, toggle);
        conditional_swap(&mut z2, &mut z3, toggle);
        swap = k_t;

        let a = x2.add(&z2);
        let b = x2.sub(&z2);
        let aa = a.square();
        let bb = b.square();
        let e = aa.sub(&bb);
        let c = x3.add(&z3);
        let d = x3.sub(&z3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        let x3_new = da.add(&cb).square();
        let z3_new = da.sub(&cb).square().mul(&x1);
        let x2_new = aa.mul(&bb);
        let a24e = e.mul(&a24);
        let aa_plus = aa.add(&a24e);
        let z2_new = e.mul(&aa_plus);

        x3 = x3_new;
        z3 = z3_new;
        x2 = x2_new;
        z2 = z2_new;
    }

    conditional_swap(&mut x2, &mut x3, swap);
    conditional_swap(&mut z2, &mut z3, swap);

    let result = x2.mul(&z2.invert());
    result.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::OsRng;

    #[test]
    fn basepoint_scalar_mult_matches_rfc7748() {
        let scalar = [
            0xa5, 0x46, 0xe3, 0x6b, 0xf0, 0x52, 0x7c, 0x9d, 0x3b, 0x16, 0x35, 0x0c, 0xa5, 0x23,
            0x12, 0x8c, 0x7f, 0x0c, 0xbc, 0x11, 0x19, 0x7c, 0x21, 0x79, 0x2a, 0xf4, 0x70, 0xf5,
            0x3a, 0x0f, 0x6a, 0x75,
        ];
        let mut clamped = scalar;
        clamp_scalar(&mut clamped);
        let result = scalar_mult(&clamped, &BASEPOINT);
        let expected = [
            0x0b, 0xf6, 0x78, 0x08, 0x82, 0x88, 0x9a, 0xe4, 0x65, 0xce, 0xe6, 0x3e, 0xf6, 0x1a,
            0xb4, 0xf8, 0x60, 0x35, 0xfc, 0x5b, 0xf1, 0xf2, 0xb7, 0xbc, 0xf9, 0xea, 0xb1, 0x13,
            0x5a, 0x34, 0x74, 0x42,
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn diffie_hellman_matches_rfc7748() {
        let alice_secret = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2,
            0x66, 0x45, 0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5,
            0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let bob_secret = [
            0x5d, 0xab, 0x08, 0x7e, 0x62, 0x4a, 0x8a, 0x4b, 0x79, 0xe1, 0x7f, 0x8b, 0x83, 0x80,
            0x0e, 0xe6, 0x6f, 0x3b, 0xb1, 0x29, 0x26, 0x18, 0xb6, 0xfd, 0x1c, 0x2f, 0x8b, 0x27,
            0xff, 0x88, 0xe0, 0xeb,
        ];
        let alice_sk = SecretKey::from_bytes(&alice_secret).unwrap();
        let bob_sk = SecretKey::from_bytes(&bob_secret).unwrap();
        let alice_pk = alice_sk.public_key();
        let bob_pk = bob_sk.public_key();
        let alice_shared = alice_sk.diffie_hellman(&bob_pk).to_bytes();
        let bob_shared = bob_sk.diffie_hellman(&alice_pk).to_bytes();
        let expected = [
            0x4a, 0x5d, 0x9d, 0x5b, 0xa4, 0xce, 0x2d, 0xe1, 0x72, 0x8e, 0x3b, 0xf4, 0x80, 0x35,
            0x0f, 0x25, 0xe0, 0x7e, 0x21, 0xc9, 0x47, 0xd1, 0x9e, 0x33, 0x76, 0xf0, 0x9b, 0x3c,
            0x1e, 0x16, 0x17, 0x42,
        ];
        assert_eq!(alice_shared, expected);
        assert_eq!(bob_shared, expected);
    }

    #[test]
    fn public_key_roundtrip_display() {
        let mut rng = OsRng::default();
        let sk = SecretKey::generate(&mut rng);
        let pk = sk.public_key();
        let encoded = pk.to_string();
        let parsed = PublicKey::from_str(&encoded).unwrap();
        assert_eq!(parsed.to_bytes(), pk.to_bytes());
    }
}
