//! Minimal first-party replacement for the `rand_core` crate.
//!
//! The implementation intentionally keeps the API surface tiny – only the
//! pieces exercised inside the workspace are provided. Additional methods can
//! be added as the migration progresses.

use std::fmt;
use std::io;

/// Error type returned when randomness sources are unavailable.
#[derive(Debug, Clone)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }
}

/// Categories of RNG errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The randomness source is unavailable on this platform.
    Unavailable,
    /// Any other placeholder failure with static context.
    Other(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::Unavailable => write!(f, "randomness source unavailable"),
            ErrorKind::Other(msg) => write!(f, "randomness error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(_: io::Error) -> Self {
        Error::new(ErrorKind::Unavailable)
    }
}

/// Core RNG trait mirrored from `rand_core`.
pub trait RngCore {
    /// Produce the next 32 bits from the RNG.
    fn next_u32(&mut self) -> u32;
    /// Produce the next 64 bits from the RNG.
    fn next_u64(&mut self) -> u64;

    /// Fill the destination slice with random bytes.
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            let len = chunk.len();
            chunk.copy_from_slice(&word[..len]);
        }
    }

    /// Fallible variant of [`fill_bytes`].
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

/// Marker trait for RNGs that provide cryptographically secure output.
pub trait CryptoRng {}

/// Stand-in for the operating system RNG.
#[derive(Debug, Clone, Copy)]
pub struct OsRng {
    state: u128,
}

impl OsRng {
    /// Construct a new OS RNG using coarse monotonic seeding.
    pub fn new() -> Result<Self, Error> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::new(ErrorKind::Unavailable))?;
        let nanos = since_epoch.as_nanos();
        Ok(Self {
            state: nanos ^ 0x9e37_79b9_7f4a_7c15_ffff,
        })
    }
}

impl RngCore for OsRng {
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() & 0xffff_ffff) as u32
    }

    fn next_u64(&mut self) -> u64 {
        // Simple xorshift128+ placeholder.
        let mut x = self.state;
        x ^= x << 23;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x ^ (x >> 32)) as u64
    }
}

impl CryptoRng for OsRng {}

/// Convenience helper mirroring the upstream constructor.
impl Default for OsRng {
    fn default() -> Self {
        OsRng::new().unwrap_or(Self {
            state: 0x1234_5678_9abc_def0,
        })
    }
}
