#![allow(clippy::needless_range_loop)]

use std::ops::Not;

/// Constant-time boolean wrapper mirroring `subtle::Choice` semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtChoice(u8);

impl CtChoice {
    /// Construct a new choice from the provided byte (only the least-significant
    /// bit is preserved).
    pub fn new(value: u8) -> Self {
        Self(value & 1)
    }

    /// Return the underlying bit as `u8` (either 0 or 1).
    pub fn as_u8(self) -> u8 {
        self.0 & 1
    }
}

impl From<bool> for CtChoice {
    fn from(value: bool) -> Self {
        if value {
            Self(1)
        } else {
            Self(0)
        }
    }
}

impl From<CtChoice> for bool {
    fn from(value: CtChoice) -> Self {
        value.as_u8() == 1
    }
}

impl Not for CtChoice {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::new(self.as_u8().not() & 1)
    }
}

/// Trait providing constant-time equality checks for byte containers.
pub trait ConstantTimeEq<Rhs: ?Sized = Self> {
    /// Perform a constant-time equality comparison.
    fn ct_eq(&self, other: &Rhs) -> CtChoice;
}

fn ct_eq_bytes(left: &[u8], right: &[u8]) -> CtChoice {
    let mut diff = (left.len() ^ right.len()) as u8;
    let max = left.len().max(right.len());
    for i in 0..max {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        diff |= a ^ b;
    }
    CtChoice::new((diff == 0) as u8)
}

impl ConstantTimeEq for [u8] {
    fn ct_eq(&self, other: &Self) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl ConstantTimeEq<Vec<u8>> for [u8] {
    fn ct_eq(&self, other: &Vec<u8>) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl ConstantTimeEq<[u8]> for Vec<u8> {
    fn ct_eq(&self, other: &[u8]) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl ConstantTimeEq for Vec<u8> {
    fn ct_eq(&self, other: &Self) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl<const N: usize> ConstantTimeEq for [u8; N] {
    fn ct_eq(&self, other: &Self) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl<const N: usize> ConstantTimeEq<[u8]> for [u8; N] {
    fn ct_eq(&self, other: &[u8]) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl<const N: usize> ConstantTimeEq<[u8; N]> for [u8] {
    fn ct_eq(&self, other: &[u8; N]) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl<const N: usize> ConstantTimeEq<Vec<u8>> for [u8; N] {
    fn ct_eq(&self, other: &Vec<u8>) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

impl<const N: usize> ConstantTimeEq<[u8; N]> for Vec<u8> {
    fn ct_eq(&self, other: &[u8; N]) -> CtChoice {
        ct_eq_bytes(self, other)
    }
}

/// Convenience helper mirroring `subtle::ConstantTimeEq::ct_eq` but returning a
/// boolean directly.
pub fn equal(left: &[u8], right: &[u8]) -> bool {
    bool::from(ct_eq_bytes(left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_equal_inputs() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 2, 3, 4];
        assert!(bool::from(a.ct_eq(&b)));
        assert!(bool::from(a[..].ct_eq(&b[..])));
        assert!(bool::from(Vec::from(a).ct_eq(&Vec::from(b))));
    }

    #[test]
    fn detects_mismatched_inputs() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 2, 3, 5];
        assert!(!bool::from(a.ct_eq(&b)));
        assert!(!bool::from(a[..].ct_eq(&b[..])));
    }

    #[test]
    fn handles_length_mismatch() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 2, 3];
        assert!(!bool::from(a[..].ct_eq(&b[..])));
        assert!(!bool::from(Vec::from(a).ct_eq(&Vec::from(b))));
    }
}
