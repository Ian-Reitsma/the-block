use num_bigint::BigUint;
use num_traits::{One, Zero};
use std::sync::OnceLock;

const BASEPOINT_X_BYTES: [u8; 32] = [
    0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9, 0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
    0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0, 0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
];

const BASEPOINT_Y_BYTES: [u8; 32] = [
    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
];

const D_BYTES: [u8; 32] = [
    0xa3, 0x78, 0x59, 0x13, 0xca, 0x4d, 0xeb, 0x75, 0xab, 0xd8, 0x41, 0x41, 0x4d, 0x0a, 0x70, 0x00,
    0x98, 0xe8, 0x79, 0x77, 0x79, 0x40, 0xc7, 0x8c, 0x73, 0xfe, 0x6f, 0x2b, 0xee, 0x6c, 0x03, 0x52,
];

fn modulus() -> &'static BigUint {
    static MODULUS: OnceLock<BigUint> = OnceLock::new();
    MODULUS.get_or_init(|| (BigUint::one() << 255u32) - BigUint::from(19u32))
}

fn sqrt_m1_value() -> &'static BigUint {
    static SQRT_M1: OnceLock<BigUint> = OnceLock::new();
    SQRT_M1.get_or_init(|| {
        let p = modulus();
        let mut exp = p.clone();
        exp -= BigUint::one();
        exp >>= 2u32;
        mod_pow(&BigUint::from(2u32), &exp, p)
    })
}

fn mod_pow(base: &BigUint, exp: &BigUint, modulus: &BigUint) -> BigUint {
    base.modpow(exp, modulus)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldElement(BigUint);

impl FieldElement {
    pub const BYTES: usize = 32;

    pub fn zero() -> FieldElement {
        FieldElement(BigUint::zero())
    }

    pub fn one() -> FieldElement {
        FieldElement(BigUint::one())
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTES]) -> Option<Self> {
        let value = BigUint::from_bytes_le(bytes);
        if &value < modulus() {
            Some(FieldElement(value))
        } else {
            None
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut bytes = self.0.to_bytes_le();
        bytes.resize(Self::BYTES, 0);
        let mut out = [0u8; Self::BYTES];
        out.copy_from_slice(&bytes);
        out
    }

    pub fn add(&self, other: &Self) -> Self {
        FieldElement::new(&self.0 + &other.0)
    }

    pub fn sub(&self, other: &Self) -> Self {
        if self.0 >= other.0 {
            FieldElement::new(&self.0 - &other.0)
        } else {
            let mut value = modulus().clone();
            value += &self.0;
            value -= &other.0;
            FieldElement::new(value)
        }
    }

    pub fn negate(&self) -> Self {
        if self.0.is_zero() {
            self.clone()
        } else {
            let mut value = modulus().clone();
            value -= &self.0;
            FieldElement::new(value)
        }
    }

    pub fn mul(&self, other: &Self) -> Self {
        FieldElement::new(&self.0 * &other.0)
    }

    pub fn square(&self) -> Self {
        FieldElement::new(&self.0 * &self.0)
    }

    pub fn invert(&self) -> Self {
        assert!(!self.0.is_zero(), "attempted to invert zero in the field");
        let mut exp = modulus().clone();
        exp -= BigUint::from(2u32);
        FieldElement::new(mod_pow(&self.0, &exp, modulus()))
    }

    pub fn is_negative(&self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }

    pub fn sqrt_ratio(u: &Self, v: &Self) -> (bool, Self) {
        let mut exp = modulus().clone();
        exp -= BigUint::from(5u32);
        exp >>= 3u32;

        let v2 = v.square();
        let v3 = v2.mul(v);
        let v4 = v2.square();
        let v7 = v3.mul(&v4);

        let uv7 = u.mul(&v7);
        let uv7_pow = FieldElement::new(mod_pow(&uv7.0, &exp, modulus()));
        let mut r = u.mul(&v3).mul(&uv7_pow);
        let check = v.mul(&r.square());

        if check == *u {
            return (true, r);
        }

        let minus_u = u.negate();
        if check == minus_u {
            let sqrt_m1 = sqrt_m1();
            r = r.mul(&sqrt_m1);
            if v.mul(&r.square()) == *u {
                return (true, r);
            }
        }

        (false, FieldElement::zero())
    }

    fn new(value: BigUint) -> Self {
        FieldElement(value % modulus())
    }
}

pub fn curve_constant_d() -> FieldElement {
    static D: OnceLock<FieldElement> = OnceLock::new();
    D.get_or_init(|| FieldElement::from_bytes(&D_BYTES).expect("valid Edwards d constant"))
        .clone()
}

pub fn base_point() -> (FieldElement, FieldElement) {
    static BASE: OnceLock<(FieldElement, FieldElement)> = OnceLock::new();
    BASE.get_or_init(|| {
        let x = FieldElement::from_bytes(&BASEPOINT_X_BYTES).expect("valid basepoint x");
        let y = FieldElement::from_bytes(&BASEPOINT_Y_BYTES).expect("valid basepoint y");
        (x, y)
    })
    .clone()
}

fn sqrt_m1() -> FieldElement {
    static SQRT_M1_FIELD: OnceLock<FieldElement> = OnceLock::new();
    SQRT_M1_FIELD
        .get_or_init(|| FieldElement::new(sqrt_m1_value().clone()))
        .clone()
}
