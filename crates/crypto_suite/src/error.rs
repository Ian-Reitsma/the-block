#![forbid(unsafe_code)]

use core::fmt;

/// Result alias used by the crypto suite when routing through stub
/// implementations.
pub type Result<T> = core::result::Result<T, Error>;

/// Minimal error type describing which crypto primitive has not yet been
/// ported to the first-party stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Unimplemented(&'static str),
}

impl Error {
    /// Construct an error pointing at the unimplemented primitive.
    pub const fn unimplemented(component: &'static str) -> Self {
        Self {
            kind: ErrorKind::Unimplemented(component),
        }
    }

    pub const fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Unimplemented(component) => {
                write!(f, "{component} is not yet implemented")
            }
        }
    }
}

impl core::error::Error for Error {}
