//! First-party coding primitives scaffold.
//!
//! Each helper currently panics or returns an error so callers become aware
//! that the in-house implementation still needs to be written. This keeps the
//! crate compiling while upstream third-party crates are removed.

/// Randomness helpers required by the encryption/fountain stacks.
pub mod rng {
    /// Error type describing which RNG operation is unavailable.
    #[derive(Debug, Clone)]
    pub struct RngError {
        pub reason: &'static str,
    }

    impl RngError {
        pub const fn unsupported(reason: &'static str) -> Self {
            Self { reason }
        }
    }

    /// Fill the buffer with secure random bytes. Currently unimplemented.
    pub fn fill_secure_bytes(dest: &mut [u8]) -> Result<(), RngError> {
        if dest.is_empty() {
            return Ok(());
        }
        Err(RngError::unsupported("secure random generation"))
    }

    /// Deterministic RNG placeholder.
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

/// Hashing helpers that currently panic.
pub mod hash {
    pub fn blake3(_data: &[u8]) -> [u8; 32] {
        unimplemented!("coding::hash::blake3 requires first-party implementation");
    }

    pub fn sha256(_data: &[u8]) -> [u8; 32] {
        unimplemented!("coding::hash::sha256 requires first-party implementation");
    }
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

/// Numeric helpers – currently placeholders.
pub mod math {
    pub fn fft(_values: &mut [Complex]) {
        unimplemented!("coding::math::fft requires first-party numeric backend");
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Complex {
        pub re: f64,
        pub im: f64,
    }
}
