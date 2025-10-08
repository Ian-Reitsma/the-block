//! First-party coding primitives.
//!
//! The module exposes randomness, hashing, encoding, and FFT helpers backed by
//! the shared sys and crypto crates so downstream encryption/erasure code can
//! operate without external dependencies.

/// Randomness helpers required by the encryption/fountain stacks.
pub mod rng {
    use std::fmt;

    use sys::{error::SysError, random};

    /// Error type describing which RNG operation is unavailable.
    #[derive(Debug)]
    pub struct RngError {
        reason: &'static str,
        source: Option<SysError>,
    }

    impl RngError {
        pub const fn unsupported(reason: &'static str) -> Self {
            Self {
                reason,
                source: None,
            }
        }

        pub fn from_sys(reason: &'static str, source: SysError) -> Self {
            Self {
                reason,
                source: Some(source),
            }
        }

        pub const fn reason(&self) -> &'static str {
            self.reason
        }
    }

    impl fmt::Display for RngError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.source {
                Some(source) => write!(f, "{reason}: {source}", reason = self.reason),
                None => write!(f, "{reason}", reason = self.reason),
            }
        }
    }

    impl std::error::Error for RngError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_ref()
                .map(|err| err as &(dyn std::error::Error + 'static))
        }
    }

    /// Fill the buffer with secure random bytes.
    pub fn fill_secure_bytes(dest: &mut [u8]) -> Result<(), RngError> {
        if dest.is_empty() {
            return Ok(());
        }
        random::fill_bytes(dest).map_err(|err| RngError::from_sys("secure random generation", err))
    }

    /// Deterministic RNG used in tests.
    #[derive(Debug, Clone, Copy)]
    pub struct DeterministicRng {
        seed: u64,
    }

    impl DeterministicRng {
        pub const fn from_seed(seed: u64) -> Self {
            Self { seed }
        }

        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.seed;
            x ^= x << 7;
            x ^= x >> 9;
            x ^= x << 13;
            self.seed = x;
            x
        }

        pub fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let word = self.next_u64().to_le_bytes();
                let len = chunk.len();
                chunk.copy_from_slice(&word[..len]);
            }
        }
    }
}

/// Hashing helpers backed by the first-party crypto crate.
pub mod hash {
    pub use crypto::primitives::hash::{
        blake3, blake3_derive_key, blake3_hash, blake3_keyed, blake3_xof, sha256, Blake3Hash,
        Blake3Hasher, Blake3HexOutput, BLAKE3_KEY_LEN, BLAKE3_OUT_LEN,
    };
}

/// Base encoders used for diagnostics and persistence.
pub mod base {
    use base64_fp::{decode_standard, encode_standard};

    pub fn encode_base64(data: &[u8]) -> String {
        encode_standard(data)
    }

    pub fn decode_base64(data: &str) -> Result<Vec<u8>, &'static str> {
        decode_standard(data).map_err(|_| "invalid base64 input")
    }
}

/// Numeric helpers including FFT.
pub mod math {
    use core::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};

    use std::f64::consts::PI;

    pub fn fft(values: &mut [Complex]) {
        let n = values.len();
        if n <= 1 {
            return;
        }
        assert!(
            n.is_power_of_two(),
            "fft input length must be a power of two"
        );

        let bits = n.trailing_zeros() as usize;
        for i in 0..n {
            let j = bit_reverse(i, bits);
            if j > i {
                values.swap(i, j);
            }
        }

        let mut len = 2;
        while len <= n {
            let angle = -2.0 * PI / len as f64;
            let w_len = Complex::from_polar(1.0, angle);
            for start in (0..n).step_by(len) {
                let mut w = Complex::one();
                for offset in 0..(len / 2) {
                    let even = values[start + offset];
                    let odd = values[start + offset + len / 2];
                    let t = w * odd;
                    values[start + offset] = even + t;
                    values[start + offset + len / 2] = even - t;
                    w *= w_len;
                }
            }
            len <<= 1;
        }
    }

    fn bit_reverse(mut value: usize, bits: usize) -> usize {
        let mut reversed = 0;
        for _ in 0..bits {
            reversed = (reversed << 1) | (value & 1);
            value >>= 1;
        }
        reversed
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Complex {
        pub re: f64,
        pub im: f64,
    }

    impl Complex {
        pub const fn new(re: f64, im: f64) -> Self {
            Self { re, im }
        }

        pub const fn one() -> Self {
            Self { re: 1.0, im: 0.0 }
        }

        fn from_polar(radius: f64, angle: f64) -> Self {
            Self {
                re: radius * angle.cos(),
                im: radius * angle.sin(),
            }
        }
    }

    impl Add for Complex {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re + rhs.re,
                im: self.im + rhs.im,
            }
        }
    }

    impl AddAssign for Complex {
        fn add_assign(&mut self, rhs: Self) {
            self.re += rhs.re;
            self.im += rhs.im;
        }
    }

    impl Sub for Complex {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re - rhs.re,
                im: self.im - rhs.im,
            }
        }
    }

    impl SubAssign for Complex {
        fn sub_assign(&mut self, rhs: Self) {
            self.re -= rhs.re;
            self.im -= rhs.im;
        }
    }

    impl Mul for Complex {
        type Output = Self;

        fn mul(self, rhs: Self) -> Self::Output {
            Self {
                re: self.re * rhs.re - self.im * rhs.im,
                im: self.re * rhs.im + self.im * rhs.re,
            }
        }
    }

    impl MulAssign for Complex {
        fn mul_assign(&mut self, rhs: Self) {
            *self = *self * rhs;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hash;
    use super::math::{self, Complex};
    use super::rng;

    #[test]
    fn secure_bytes_are_generated() {
        let mut buf = [0u8; 16];
        rng::fill_secure_bytes(&mut buf).expect("secure bytes");
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn blake3_matches_reference() {
        const EXPECTED: [u8; 32] = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62,
        ];
        assert_eq!(hash::blake3(b""), EXPECTED);
    }

    #[test]
    fn sha256_matches_reference() {
        const EXPECTED: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(hash::sha256(b"abc"), EXPECTED);
    }

    #[test]
    fn fft_rounds_trip_impulse() {
        let mut values = [
            Complex::new(1.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.0, 0.0),
        ];
        math::fft(&mut values);
        for value in values.iter() {
            approx(value.re, 1.0);
            approx(value.im, 0.0);
        }
    }

    fn approx(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }
}
