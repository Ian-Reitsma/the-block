use super::field::{base_point, curve_constant_d, FieldElement};
use super::scalar::Scalar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressedPoint(pub [u8; 32]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdwardsPoint {
    pub(crate) x: FieldElement,
    pub(crate) y: FieldElement,
}

impl CompressedPoint {
    pub fn decompress(&self) -> Option<EdwardsPoint> {
        let mut bytes = self.0;
        let sign = (bytes[31] >> 7) & 1;
        bytes[31] &= 0x7f;
        let y = FieldElement::from_bytes(&bytes)?;
        let y_sq = y.square();
        let u = y_sq.sub(&FieldElement::one());
        let v = curve_constant_d().mul(&y_sq).add(&FieldElement::one());
        let (was_square, mut x) = FieldElement::sqrt_ratio(&u, &v);
        if !was_square {
            return None;
        }
        if x.is_negative() != (sign == 1) {
            x = x.negate();
        }
        let point = EdwardsPoint { x, y };
        if point.is_small_order() {
            return None;
        }
        Some(point)
    }
}

impl EdwardsPoint {
    pub fn identity() -> Self {
        Self {
            x: FieldElement::zero(),
            y: FieldElement::one(),
        }
    }

    pub fn basepoint() -> Self {
        let (x, y) = base_point();
        Self { x, y }
    }

    pub fn compress(&self) -> [u8; 32] {
        let mut bytes = self.y.to_bytes();
        if self.x.is_negative() {
            bytes[31] |= 1 << 7;
        }
        bytes
    }

    pub fn add(&self, other: &Self) -> Self {
        let d = curve_constant_d();
        let x1x2 = self.x.mul(&other.x);
        let y1y2 = self.y.mul(&other.y);
        let xyxy = d.mul(&x1x2).mul(&y1y2);
        let numerator_x = self.x.mul(&other.y).add(&self.y.mul(&other.x));
        let numerator_y = y1y2.add(&x1x2);
        let denom_x = FieldElement::one().add(&xyxy).invert();
        let denom_y = FieldElement::one().sub(&xyxy).invert();
        let x3 = numerator_x.mul(&denom_x);
        let y3 = numerator_y.mul(&denom_y);
        EdwardsPoint { x: x3, y: y3 }
    }

    pub fn scalar_mul(&self, scalar: &Scalar) -> Self {
        let mut result = EdwardsPoint::identity();
        let mut base = self.clone();
        let mut k = scalar.to_bytes();
        for byte in k.iter_mut() {
            let mut b = *byte;
            for _ in 0..8 {
                if (b & 1) == 1 {
                    result = result.add(&base);
                }
                base = base.add(&base);
                b >>= 1;
            }
        }
        result
    }

    pub fn mul_base(scalar: &Scalar) -> Self {
        EdwardsPoint::basepoint().scalar_mul(scalar)
    }

    pub fn is_small_order(&self) -> bool {
        let mut point = self.clone();
        for _ in 0..3 {
            point = point.add(&point);
        }
        point == EdwardsPoint::identity()
    }
}
