use foundation_bigint::BigUint;
use std::sync::OnceLock;

const L_BYTES: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

fn modulus_l() -> &'static BigUint {
    static L: OnceLock<BigUint> = OnceLock::new();
    L.get_or_init(|| BigUint::from_bytes_le(&L_BYTES))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scalar(BigUint);

impl Scalar {
    pub const BYTES: usize = 32;

    pub fn from_bytes_mod_order(bytes: &[u8; Self::BYTES]) -> Self {
        Scalar::new(BigUint::from_bytes_le(bytes))
    }

    pub fn from_bytes_mod_order_wide(bytes: &[u8; 64]) -> Self {
        Scalar::new(BigUint::from_bytes_le(bytes))
    }

    pub fn from_canonical_bytes(bytes: &[u8; Self::BYTES]) -> Option<Self> {
        let value = BigUint::from_bytes_le(bytes);
        if &value < modulus_l() {
            Some(Scalar(value))
        } else {
            None
        }
    }

    pub fn is_canonical(bytes: &[u8; Self::BYTES]) -> bool {
        let value = BigUint::from_bytes_le(bytes);
        &value < modulus_l()
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut bytes = self.0.to_bytes_le();
        bytes.resize(Self::BYTES, 0);
        let mut out = [0u8; Self::BYTES];
        out.copy_from_slice(&bytes);
        out
    }

    pub fn mul_add(a: &Self, b: &Self, c: &Self) -> Self {
        let value = &a.0 * &b.0 + &c.0;
        Scalar::new(value)
    }

    fn new(value: BigUint) -> Self {
        Scalar(value % modulus_l())
    }
}
