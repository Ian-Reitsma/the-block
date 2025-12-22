//! Integer helper traits mirroring the pieces of `num-traits`/`num-integer`
//! that the workspace relied on before they were excised.

use core::fmt::Debug;

/// Trait exposing a canonical zero value and an `is_zero` predicate.
pub trait Zero: Sized + PartialEq {
    /// Return the additive identity for the type.
    fn zero() -> Self;

    /// Returns `true` when the value equals zero.
    fn is_zero(&self) -> bool;
}

/// Trait exposing a canonical one value.
pub trait One: Sized {
    /// Return the multiplicative identity for the type.
    fn one() -> Self;
}

/// Minimal integer helper trait providing the handful of routines we relied on
/// from `num_integer::Integer`.
pub trait Integer: Copy + Zero + One + PartialOrd + Debug {
    /// Integer division rounding towards negative infinity.
    fn div_floor(self, rhs: Self) -> Self;

    /// Integer division rounding towards positive infinity.
    fn div_ceil(self, rhs: Self) -> Self;

    /// Greatest common divisor computed via Euclid's algorithm.
    fn gcd(self, rhs: Self) -> Self;
}

macro_rules! impl_zero_one_unsigned {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl Zero for $ty {
                fn zero() -> Self {
                    0
                }

                fn is_zero(&self) -> bool {
                    *self == 0
                }
            }

            impl One for $ty {
                fn one() -> Self {
                    1
                }
            }

            impl Integer for $ty {
                fn div_floor(self, rhs: Self) -> Self {
                    assert!(rhs != 0, "division by zero");
                    self / rhs
                }

                fn div_ceil(self, rhs: Self) -> Self {
                    assert!(rhs != 0, "division by zero");
                    if self == 0 {
                        return 0;
                    }
                    let div = self / rhs;
                    if self % rhs == 0 {
                        div
                    } else {
                        div + 1
                    }
                }

                fn gcd(self, rhs: Self) -> Self {
                    let mut a = self;
                    let mut b = rhs;
                    while b != 0 {
                        let t = b;
                        b = a % b;
                        a = t;
                    }
                    a
                }
            }
        )+
    };
}

macro_rules! impl_zero_one_signed {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl Zero for $ty {
                fn zero() -> Self {
                    0
                }

                fn is_zero(&self) -> bool {
                    *self == 0
                }
            }

            impl One for $ty {
                fn one() -> Self {
                    1
                }
            }

            impl Integer for $ty {
                fn div_floor(self, rhs: Self) -> Self {
                    assert!(rhs != 0, "division by zero");
                    let q = self / rhs;
                    let r = self % rhs;
                    if r == 0 {
                        q
                    } else if (r > 0 && rhs < 0) || (r < 0 && rhs > 0) {
                        q - 1
                    } else {
                        q
                    }
                }

                fn div_ceil(self, rhs: Self) -> Self {
                    assert!(rhs != 0, "division by zero");
                    let q = self / rhs;
                    let r = self % rhs;
                    if r == 0 {
                        q
                    } else if (r > 0 && rhs > 0) || (r < 0 && rhs < 0) {
                        q + 1
                    } else {
                        q
                    }
                }

                fn gcd(self, rhs: Self) -> Self {
                    let mut a = self.abs();
                    let mut b = rhs.abs();
                    while b != 0 {
                        let t = b;
                        b = a % b;
                        a = t;
                    }
                    a
                }
            }
        )+
    };
}

impl_zero_one_unsigned!(u8, u16, u32, u64, u128, usize);
impl_zero_one_signed!(i8, i16, i32, i64, i128, isize);

#[cfg(test)]
mod tests {
    use super::{Integer, One, Zero};

    #[test]
    fn unsigned_helpers() {
        assert_eq!(u32::zero(), 0);
        assert!(u32::zero().is_zero());
        assert_eq!(u32::one(), 1);
        assert_eq!(Integer::div_floor(10u32, 3), 3);
        assert_eq!(Integer::div_ceil(10u32, 3), 4);
        assert_eq!(Integer::div_floor(10u32, 5), 2);
        assert_eq!(Integer::div_ceil(10u32, 5), 2);
        assert_eq!(Integer::gcd(24u32, 18), 6);
    }

    #[test]
    fn signed_helpers() {
        assert_eq!(Integer::div_floor(-7i32, 2), -4);
        assert_eq!(Integer::div_ceil(-7i32, 2), -3);
        assert_eq!(Integer::div_floor(7i32, -2), -4);
        assert_eq!(Integer::div_ceil(7i32, -2), -3);
        assert_eq!(Integer::gcd(-24i32, 18), 6);
    }
}
