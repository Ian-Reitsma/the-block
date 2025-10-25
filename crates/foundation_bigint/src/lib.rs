#![forbid(unsafe_code)]

use core::cmp::Ordering;
use core::fmt;
use core::ops::{
    Add, AddAssign, Mul, MulAssign, Rem, RemAssign, Shl, ShlAssign, Shr, ShrAssign, Sub, SubAssign,
};

const LIMB_BITS: u32 = 32;
const LIMB_BASE: u64 = 1u64 << LIMB_BITS;

/// Collection of helper traits that mirror the small subset of `num_traits`
/// required by the crypto suite.  These helpers are intentionally tiny so the
/// bigint implementation can stand on its own without pulling additional
/// crates into FIRST_PARTY builds.
pub mod traits {
    /// Trait exposing a canonical zero value and an `is_zero` predicate.
    pub trait Zero {
        /// Return the zero value for the implementing type.
        fn zero() -> Self;
        /// Returns `true` when the value equals zero.
        fn is_zero(&self) -> bool;
    }

    /// Trait exposing a canonical one value.
    pub trait One {
        /// Return the multiplicative identity.
        fn one() -> Self;
    }
}

/// Unsigned arbitrary-precision integer backed by base-2^32 limbs stored in
/// little-endian order (least-significant limb first).
#[derive(Clone, Default, Eq, PartialEq)]
pub struct BigUint {
    digits: Vec<u32>,
}

impl BigUint {
    /// Construct zero.
    #[must_use]
    pub fn zero() -> Self {
        Self { digits: Vec::new() }
    }

    /// Construct one.
    #[must_use]
    pub fn one() -> Self {
        Self { digits: vec![1] }
    }

    /// Returns `true` when the value is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    /// Returns `true` when the value is odd.
    #[must_use]
    pub fn is_odd(&self) -> bool {
        !self.is_zero() && (self.digits[0] & 1) == 1
    }

    /// Normalize the digit buffer by trimming high zero limbs.
    fn normalize(&mut self) {
        while matches!(self.digits.last(), Some(0)) {
            self.digits.pop();
        }
    }

    /// Construct from little-endian bytes.
    #[must_use]
    pub fn from_bytes_le(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::zero();
        }
        let mut digits = Vec::with_capacity(bytes.len().div_ceil(4));
        for chunk in bytes.chunks(4) {
            let mut limb = 0u32;
            for (shift, byte) in chunk.iter().enumerate() {
                limb |= (*byte as u32) << (shift * 8);
            }
            digits.push(limb);
        }
        let mut value = Self { digits };
        value.normalize();
        value
    }

    /// Construct from big-endian bytes.
    #[must_use]
    pub fn from_bytes_be(bytes: &[u8]) -> Self {
        let mut reversed = bytes.to_vec();
        reversed.reverse();
        Self::from_bytes_le(&reversed)
    }

    /// Return the little-endian byte representation with the minimal number of
    /// limbs.  Zero is encoded as a single zero byte to match the behaviour of
    /// existing bigint shims.
    #[must_use]
    pub fn to_bytes_le(&self) -> Vec<u8> {
        if self.is_zero() {
            return vec![0];
        }
        let mut bytes = Vec::with_capacity(self.digits.len() * 4);
        for limb in &self.digits {
            bytes.push((*limb & 0xFF) as u8);
            bytes.push(((*limb >> 8) & 0xFF) as u8);
            bytes.push(((*limb >> 16) & 0xFF) as u8);
            bytes.push(((*limb >> 24) & 0xFF) as u8);
        }
        while matches!(bytes.last(), Some(0)) {
            bytes.pop();
        }
        bytes
    }

    /// Return the big-endian byte representation.
    #[must_use]
    pub fn to_bytes_be(&self) -> Vec<u8> {
        let mut bytes = self.to_bytes_le();
        bytes.reverse();
        bytes
    }

    /// Parse bytes interpreted in the provided radix.  Accepts radices in the
    /// `[2, 36]` range.
    #[must_use]
    pub fn parse_bytes(bytes: &[u8], radix: u32) -> Option<Self> {
        if !(2..=36).contains(&radix) {
            return None;
        }
        if bytes.is_empty() {
            return Some(Self::zero());
        }
        let mut value = Self::zero();
        for &byte in bytes {
            let digit = match byte {
                b'0'..=b'9' => (byte - b'0') as u32,
                b'a'..=b'z' => (byte - b'a' + 10) as u32,
                b'A'..=b'Z' => (byte - b'A' + 10) as u32,
                _ => return None,
            };
            if digit >= radix {
                return None;
            }
            value.mul_assign_small(radix);
            value.add_assign_small(digit);
        }
        value.normalize();
        Some(value)
    }

    /// Modular exponentiation by repeated squaring.
    #[must_use]
    pub fn modpow(&self, exponent: &Self, modulus: &Self) -> Self {
        assert!(!modulus.is_zero(), "modulus must be non-zero");
        if modulus.is_one() {
            return Self::zero();
        }
        let mut base = self % modulus;
        let mut exp = exponent.clone();
        let mut acc = Self::one();
        while !exp.is_zero() {
            if exp.is_odd() {
                acc = (&acc * &base) % modulus;
            }
            exp >>= 1u32;
            if exp.is_zero() {
                break;
            }
            base = (&base * &base) % modulus;
        }
        acc
    }

    fn is_one(&self) -> bool {
        self.digits == [1]
    }

    fn cmp_magnitude(&self, other: &Self) -> Ordering {
        match self.digits.len().cmp(&other.digits.len()) {
            Ordering::Equal => {
                for (a, b) in self.digits.iter().zip(other.digits.iter()).rev() {
                    match a.cmp(b) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                }
                Ordering::Equal
            }
            non_eq => non_eq,
        }
    }

    fn add_assign_small(&mut self, rhs: u32) {
        let mut carry = rhs as u64;
        for digit in &mut self.digits {
            let sum = *digit as u64 + carry;
            *digit = sum as u32;
            carry = sum >> LIMB_BITS;
            if carry == 0 {
                break;
            }
        }
        if carry != 0 {
            self.digits.push(carry as u32);
        }
    }

    fn mul_assign_small(&mut self, rhs: u32) {
        if self.is_zero() || rhs == 1 {
            return;
        }
        if rhs == 0 {
            self.digits.clear();
            return;
        }
        let mut carry = 0u64;
        for digit in &mut self.digits {
            let product = (*digit as u64) * rhs as u64 + carry;
            *digit = product as u32;
            carry = product >> LIMB_BITS;
        }
        if carry != 0 {
            self.digits.push(carry as u32);
        }
    }

    fn shl_bits_assign(&mut self, bits: u32) {
        if self.is_zero() {
            return;
        }
        let limb_shift = (bits / LIMB_BITS) as usize;
        let bit_shift = bits % LIMB_BITS;
        if bit_shift == 0 {
            self.digits.splice(0..0, std::iter::repeat_n(0, limb_shift));
            return;
        }
        let mut new_digits = vec![0u32; self.digits.len() + limb_shift + 1];
        let mut carry = 0u32;
        for (idx, &digit) in self.digits.iter().enumerate() {
            let combined = ((digit as u64) << bit_shift) | carry as u64;
            new_digits[idx + limb_shift] = combined as u32;
            carry = (combined >> LIMB_BITS) as u32;
        }
        if carry != 0 {
            new_digits[self.digits.len() + limb_shift] = carry;
        }
        let mut start = new_digits.len();
        while start > 0 && new_digits[start - 1] == 0 {
            start -= 1;
        }
        new_digits.truncate(start);
        self.digits = new_digits;
    }

    fn shr_bits_assign(&mut self, bits: u32) {
        if self.is_zero() {
            return;
        }
        let limb_shift = (bits / LIMB_BITS) as usize;
        let bit_shift = bits % LIMB_BITS;
        if limb_shift >= self.digits.len() {
            self.digits.clear();
            return;
        }
        let mut new_digits = self.digits[limb_shift..].to_vec();
        if bit_shift == 0 {
            self.digits = new_digits;
            self.normalize();
            return;
        }
        let mask = (1u32 << bit_shift) - 1;
        let mut carry = 0u32;
        for digit in new_digits.iter_mut().rev() {
            let new_carry = (*digit & mask) << (LIMB_BITS - bit_shift);
            *digit = (*digit >> bit_shift) | carry;
            carry = new_carry;
        }
        let mut value = Self { digits: new_digits };
        value.normalize();
        self.digits = value.digits;
    }

    fn add_ref(lhs: &Self, rhs: &Self) -> Self {
        let mut result = lhs.clone();
        if result.digits.len() < rhs.digits.len() {
            result.digits.resize(rhs.digits.len(), 0);
        }
        let mut carry = 0u64;
        for idx in 0..result.digits.len() {
            let rhs_digit = rhs.digits.get(idx).copied().unwrap_or(0) as u64;
            let sum = result.digits[idx] as u64 + rhs_digit + carry;
            result.digits[idx] = sum as u32;
            carry = sum >> LIMB_BITS;
        }
        if carry != 0 {
            result.digits.push(carry as u32);
        }
        result
    }

    fn sub_ref(lhs: &Self, rhs: &Self) -> Self {
        assert!(lhs >= rhs, "BigUint subtraction underflow");
        let mut result = lhs.clone();
        let mut borrow = 0i64;
        for idx in 0..result.digits.len() {
            let lhs_digit = result.digits[idx] as i64;
            let rhs_digit = rhs.digits.get(idx).copied().unwrap_or(0) as i64;
            let mut value = lhs_digit - rhs_digit - borrow;
            if value < 0 {
                value += LIMB_BASE as i64;
                borrow = 1;
            } else {
                borrow = 0;
            }
            result.digits[idx] = value as u32;
        }
        assert_eq!(borrow, 0, "BigUint subtraction borrow remained");
        result.normalize();
        result
    }

    fn mul_ref(lhs: &Self, rhs: &Self) -> Self {
        if lhs.is_zero() || rhs.is_zero() {
            return Self::zero();
        }
        let mut result = vec![0u32; lhs.digits.len() + rhs.digits.len()];
        for (i, &ld) in lhs.digits.iter().enumerate() {
            let mut carry = 0u128;
            for (j, &rd) in rhs.digits.iter().enumerate() {
                let idx = i + j;
                let product = (ld as u128) * (rd as u128) + result[idx] as u128 + carry;
                result[idx] = product as u32;
                carry = product >> LIMB_BITS;
            }
            if carry != 0 {
                result[i + rhs.digits.len()] =
                    (result[i + rhs.digits.len()] as u128 + carry) as u32;
            }
        }
        let mut value = Self { digits: result };
        value.normalize();
        value
    }

    fn div_rem(mut dividend: Self, divisor: &Self) -> (Self, Self) {
        assert!(!divisor.is_zero(), "division by zero");
        if dividend < *divisor {
            return (Self::zero(), dividend);
        }
        if divisor.digits.len() == 1 {
            let (quot, rem) = dividend.div_rem_small(divisor.digits[0]);
            return (quot, Self::from(rem));
        }

        let shift = divisor.digits.last().unwrap().leading_zeros();
        let mut divisor_norm = divisor.clone();
        if shift != 0 {
            divisor_norm.shl_bits_assign(shift);
        }
        let mut dividend_norm = dividend.clone();
        if shift != 0 {
            dividend_norm.shl_bits_assign(shift);
        }
        dividend_norm.digits.push(0);

        let n = divisor_norm.digits.len();
        let m = dividend_norm.digits.len() - n;
        let mut quotient = vec![0u32; m];

        for j in (0..m).rev() {
            let high = dividend_norm.digits[j + n] as u64;
            let mid = dividend_norm.digits[j + n - 1] as u64;
            let low = dividend_norm.digits[j + n - 2] as u64;
            let numerator = ((high << LIMB_BITS) | mid) as u128;
            let denom = divisor_norm.digits[n - 1] as u64 as u128;
            let mut q_hat = numerator / denom;
            if q_hat >= (1u128 << LIMB_BITS) {
                q_hat = (1u128 << LIMB_BITS) - 1;
            }
            let mut r_hat = numerator % denom;
            let second = if n >= 2 {
                divisor_norm.digits[n - 2] as u64
            } else {
                0
            };
            while q_hat * second as u128 > ((r_hat << LIMB_BITS) | low as u128) {
                q_hat -= 1;
                r_hat += denom;
                if r_hat >= (1u128 << LIMB_BITS) {
                    break;
                }
            }

            if Self::sub_mul_at(
                &mut dividend_norm.digits,
                j,
                &divisor_norm.digits,
                q_hat as u32,
            ) {
                Self::add_at(&mut dividend_norm.digits, j, &divisor_norm.digits);
                q_hat -= 1;
            }
            quotient[j] = q_hat as u32;
        }

        let remainder_digits = dividend_norm.digits[..n].to_vec();
        let mut remainder = Self {
            digits: remainder_digits,
        };
        if shift != 0 {
            remainder.shr_bits_assign(shift);
        }
        remainder.normalize();

        let mut quotient_value = Self { digits: quotient };
        quotient_value.normalize();
        (quotient_value, remainder)
    }

    fn sub_mul_at(dst: &mut [u32], start: usize, src: &[u32], multiplier: u32) -> bool {
        let mut carry = 0u128;
        let mut borrow = 0i64;
        for (offset, &source_digit) in src.iter().enumerate() {
            let idx = start + offset;
            let product = (source_digit as u128) * (multiplier as u128) + carry;
            carry = product >> LIMB_BITS;
            let product_low = product as u32;
            let value = dst[idx] as i64 - product_low as i64 - borrow;
            if value < 0 {
                dst[idx] = (value + LIMB_BASE as i64) as u32;
                borrow = 1;
            } else {
                dst[idx] = value as u32;
                borrow = 0;
            }
        }
        let idx = start + src.len();
        let value = dst[idx] as i64 - carry as i64 - borrow;
        if value < 0 {
            dst[idx] = (value + LIMB_BASE as i64) as u32;
            true
        } else {
            dst[idx] = value as u32;
            false
        }
    }

    fn add_at(dst: &mut [u32], start: usize, src: &[u32]) {
        let mut carry = 0u64;
        for (offset, &source_digit) in src.iter().enumerate() {
            let idx = start + offset;
            let sum = dst[idx] as u64 + source_digit as u64 + carry;
            dst[idx] = sum as u32;
            carry = sum >> LIMB_BITS;
        }
        let mut idx = start + src.len();
        while carry != 0 {
            let sum = dst[idx] as u64 + carry;
            dst[idx] = sum as u32;
            carry = sum >> LIMB_BITS;
            idx += 1;
        }
    }

    fn div_rem_small(&mut self, divisor: u32) -> (Self, u32) {
        assert!(divisor != 0, "division by zero");
        if self.is_zero() {
            return (Self::zero(), 0);
        }
        let mut quotient = vec![0u32; self.digits.len()];
        let mut remainder = 0u64;
        for (idx, &digit) in self.digits.iter().enumerate().rev() {
            let acc = (remainder << LIMB_BITS) | digit as u64;
            let q = acc / divisor as u64;
            let r = acc % divisor as u64;
            quotient[idx] = q as u32;
            remainder = r;
        }
        let mut value = Self { digits: quotient };
        value.normalize();
        (value, remainder as u32)
    }
}

impl traits::Zero for BigUint {
    fn zero() -> Self {
        Self::zero()
    }

    fn is_zero(&self) -> bool {
        self.is_zero()
    }
}

impl traits::One for BigUint {
    fn one() -> Self {
        Self::one()
    }
}

impl From<u8> for BigUint {
    fn from(value: u8) -> Self {
        Self::from(value as u32)
    }
}

impl From<u16> for BigUint {
    fn from(value: u16) -> Self {
        Self::from(value as u32)
    }
}

impl From<u32> for BigUint {
    fn from(value: u32) -> Self {
        if value == 0 {
            Self::zero()
        } else {
            Self {
                digits: vec![value],
            }
        }
    }
}

impl From<u64> for BigUint {
    fn from(value: u64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let lower = value as u32;
        let upper = (value >> LIMB_BITS) as u32;
        let mut digits = vec![lower];
        if upper != 0 {
            digits.push(upper);
        }
        Self { digits }
    }
}

impl From<u128> for BigUint {
    fn from(value: u128) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let mut digits = Vec::new();
        let mut remaining = value;
        while remaining != 0 {
            digits.push((remaining & ((1u128 << LIMB_BITS) - 1)) as u32);
            remaining >>= LIMB_BITS;
        }
        Self { digits }
    }
}

impl From<usize> for BigUint {
    fn from(value: usize) -> Self {
        Self::from(value as u64)
    }
}

impl fmt::Display for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        let mut value = self.clone();
        let mut parts = Vec::new();
        let base = 1_000_000_000u32;
        while !value.is_zero() {
            let (quotient, remainder) = value.div_rem_small(base);
            parts.push(remainder);
            value = quotient;
        }
        if let Some(last) = parts.pop() {
            write!(f, "{}", last)?;
        }
        while let Some(part) = parts.pop() {
            write!(f, "{:09}", part)?;
        }
        Ok(())
    }
}

impl fmt::Debug for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BigUint({})", self)
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_magnitude(other)
    }
}

impl<'a> Add<&'a BigUint> for BigUint {
    type Output = BigUint;

    fn add(self, rhs: &'a BigUint) -> Self::Output {
        Self::add_ref(&self, rhs)
    }
}

impl Add<BigUint> for BigUint {
    type Output = BigUint;

    fn add(mut self, rhs: BigUint) -> Self::Output {
        self += &rhs;
        self
    }
}

impl<'a> Add<&'a BigUint> for &'a BigUint {
    type Output = BigUint;

    fn add(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::add_ref(self, rhs)
    }
}

impl AddAssign<&BigUint> for BigUint {
    fn add_assign(&mut self, rhs: &BigUint) {
        *self = BigUint::add_ref(self, rhs);
    }
}

impl AddAssign<BigUint> for BigUint {
    fn add_assign(&mut self, rhs: BigUint) {
        *self += &rhs;
    }
}

impl<'a> Sub<&'a BigUint> for BigUint {
    type Output = BigUint;

    fn sub(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::sub_ref(&self, rhs)
    }
}

impl Sub<BigUint> for BigUint {
    type Output = BigUint;

    fn sub(mut self, rhs: BigUint) -> Self::Output {
        self -= &rhs;
        self
    }
}

impl<'a> Sub<&'a BigUint> for &'a BigUint {
    type Output = BigUint;

    fn sub(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::sub_ref(self, rhs)
    }
}

impl SubAssign<&BigUint> for BigUint {
    fn sub_assign(&mut self, rhs: &BigUint) {
        *self = BigUint::sub_ref(self, rhs);
    }
}

impl SubAssign<BigUint> for BigUint {
    fn sub_assign(&mut self, rhs: BigUint) {
        *self -= &rhs;
    }
}

impl<'a> Mul<&'a BigUint> for BigUint {
    type Output = BigUint;

    fn mul(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::mul_ref(&self, rhs)
    }
}

impl Mul<BigUint> for BigUint {
    type Output = BigUint;

    fn mul(mut self, rhs: BigUint) -> Self::Output {
        self *= &rhs;
        self
    }
}

impl<'a> Mul<&'a BigUint> for &'a BigUint {
    type Output = BigUint;

    fn mul(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::mul_ref(self, rhs)
    }
}

impl MulAssign<&BigUint> for BigUint {
    fn mul_assign(&mut self, rhs: &BigUint) {
        *self = BigUint::mul_ref(self, rhs);
    }
}

impl MulAssign<BigUint> for BigUint {
    fn mul_assign(&mut self, rhs: BigUint) {
        *self *= &rhs;
    }
}

impl<'a> Rem<&'a BigUint> for BigUint {
    type Output = BigUint;

    fn rem(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::div_rem(self, rhs).1
    }
}

impl Rem<BigUint> for BigUint {
    type Output = BigUint;

    fn rem(self, rhs: BigUint) -> Self::Output {
        BigUint::div_rem(self, &rhs).1
    }
}

impl<'a> Rem<&'a BigUint> for &'a BigUint {
    type Output = BigUint;

    fn rem(self, rhs: &'a BigUint) -> Self::Output {
        BigUint::div_rem(self.clone(), rhs).1
    }
}

impl RemAssign<&BigUint> for BigUint {
    fn rem_assign(&mut self, rhs: &BigUint) {
        *self = BigUint::div_rem(self.clone(), rhs).1;
    }
}

impl RemAssign<BigUint> for BigUint {
    fn rem_assign(&mut self, rhs: BigUint) {
        *self %= &rhs;
    }
}

impl Shl<u32> for BigUint {
    type Output = BigUint;

    fn shl(mut self, rhs: u32) -> Self::Output {
        self.shl_bits_assign(rhs);
        self
    }
}

impl Shl<u32> for &BigUint {
    type Output = BigUint;

    fn shl(self, rhs: u32) -> Self::Output {
        let mut value = self.clone();
        value.shl_bits_assign(rhs);
        value
    }
}

impl ShlAssign<u32> for BigUint {
    fn shl_assign(&mut self, rhs: u32) {
        self.shl_bits_assign(rhs);
    }
}

impl Shr<u32> for BigUint {
    type Output = BigUint;

    fn shr(mut self, rhs: u32) -> Self::Output {
        self.shr_bits_assign(rhs);
        self
    }
}

impl Shr<u32> for &BigUint {
    type Output = BigUint;

    fn shr(self, rhs: u32) -> Self::Output {
        let mut value = self.clone();
        value.shr_bits_assign(rhs);
        value
    }
}

impl ShrAssign<u32> for BigUint {
    fn shr_assign(&mut self, rhs: u32) {
        self.shr_bits_assign(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::BigUint;

    #[test]
    fn display_round_trip() {
        let value = BigUint::parse_bytes(b"12345678901234567890", 10).unwrap();
        assert_eq!(value.to_string(), "12345678901234567890");
    }

    #[test]
    fn addition_and_subtraction() {
        let a = BigUint::from(123456789u64);
        let b = BigUint::from(987654321u64);
        let sum = &a + &b;
        let diff = &sum - &a;
        assert_eq!(diff, b);
    }

    #[test]
    fn multiplication() {
        let a = BigUint::from(1_000_000_000u64);
        let b = BigUint::from(123456789u64);
        let product = &a * &b;
        assert_eq!(product.to_string(), "123456789000000000");
    }

    #[test]
    fn division_small() {
        let value = BigUint::parse_bytes(b"12345678901234567890", 10).unwrap();
        let modulus = BigUint::from(97u32);
        let remainder = &value % &modulus;
        assert_eq!(remainder.to_string(), "3");
    }

    #[test]
    fn modpow_matches_reference() {
        let base = BigUint::parse_bytes(b"12345678901234567890", 10).unwrap();
        let exponent = BigUint::from(65537u32);
        let modulus = BigUint::parse_bytes(b"4294967291", 10).unwrap();
        let result = base.modpow(&exponent, &modulus);
        assert!(result < modulus);
    }

    #[test]
    fn zero_and_one_traits() {
        assert!(BigUint::zero().is_zero());
        assert_eq!(BigUint::one(), BigUint::from(1u32));
    }
}
