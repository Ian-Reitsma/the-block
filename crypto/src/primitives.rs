//! First-party crypto primitives scaffolding.
//!
//! The current implementations are stubs that intentionally panic when
//! invoked. They let the rest of the workspace compile without relying on
//! external crates while dedicated first-party backends are implemented.

/// Deterministic and entropy-backed randomness utilities.
pub mod rng {
    /// Error returned when an RNG operation is not yet implemented.
    #[derive(Debug, Clone)]
    pub struct RngError {
        pub context: &'static str,
    }

    impl RngError {
        pub const fn unsupported(context: &'static str) -> Self {
            Self { context }
        }
    }

    /// Placeholder stand-in for OS-provided secure randomness.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct OsRng;

    impl OsRng {
        /// Fill the destination buffer with secure random bytes.
        pub fn fill_bytes(&mut self, _dest: &mut [u8]) {
            unimplemented!("OsRng::fill_bytes requires first-party entropy source");
        }

        /// Generate the next 64 bits of randomness.
        pub fn next_u64(&mut self) -> u64 {
            unimplemented!("OsRng::next_u64 requires first-party entropy source");
        }
    }

    /// Deterministic RNG used for reproducible testing.
    #[derive(Debug, Clone, Copy)]
    pub struct DeterministicRng {
        seed: u64,
    }

    impl DeterministicRng {
        /// Construct a new deterministic RNG from a seed value.
        pub const fn from_seed(seed: u64) -> Self {
            Self { seed }
        }

        /// Produce the next 64-bit value from the deterministic stream.
        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.seed;
            // Simple xorshift placeholder – replace with vetted algorithm.
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.seed = x;
            x
        }

        /// Fill the provided buffer with pseudorandom bytes.
        pub fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let value = self.next_u64().to_le_bytes();
                let len = chunk.len();
                chunk.copy_from_slice(&value[..len]);
            }
        }
    }
}

/// Hashing helpers that will eventually host first-party implementations.
pub mod hash {
    /// Placeholder Blake3 hasher entry point.
    pub fn blake3(_data: &[u8]) -> [u8; 32] {
        unimplemented!("blake3 hashing requires in-house backend");
    }

    /// Placeholder SHA-256 helper.
    pub fn sha256(_data: &[u8]) -> [u8; 32] {
        unimplemented!("sha256 hashing requires in-house backend");
    }
}

/// Base-N encoders used across the stack.
pub mod base {
    use base64_fp::{decode_standard, encode_standard};

    /// Encode bytes as Base64.
    pub fn encode_base64(input: &[u8]) -> String {
        encode_standard(input)
    }

    /// Decode Base64 text into raw bytes.
    pub fn decode_base64(input: &str) -> Result<Vec<u8>, &'static str> {
        decode_standard(input).map_err(|_| "invalid base64 input")
    }
}

/// Numeric helpers that currently panic until implemented.
pub mod math {
    /// Placeholder FFT routine used by polynomial commitments.
    pub fn fft(_input: &mut [Complex]) {
        unimplemented!("fft routine requires in-house numeric backend");
    }

    /// Minimal complex number placeholder.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Complex {
        pub re: f64,
        pub im: f64,
    }
}
