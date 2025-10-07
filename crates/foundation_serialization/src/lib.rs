#![forbid(unsafe_code)]

use std::fmt;

/// Result alias used by the stub serialization routines.
pub type Result<T> = std::result::Result<T, Error>;

/// Minimal error type describing which serialization backend is unimplemented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Unimplemented(&'static str),
}

impl Error {
    /// Construct an error that highlights the missing component.
    pub const fn unimplemented(component: &'static str) -> Self {
        Self {
            kind: ErrorKind::Unimplemented(component),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Unimplemented(component) => {
                write!(f, "{component} serialization is not yet implemented")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Trait describing types that can be encoded by the first-party stack.
pub trait Encode {
    fn encode<W: Writer>(&self, writer: &mut W) -> Result<()>;
}

/// Trait describing types that can be decoded by the first-party stack.
pub trait Decode: Sized {
    fn decode<R: Reader>(reader: &mut R) -> Result<Self>;
}

/// Stub writer that currently accumulates bytes in memory.
pub trait Writer {
    fn write(&mut self, bytes: &[u8]) -> Result<()>;
}

/// Stub reader trait for symmetry with [`Writer`].
pub trait Reader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}

/// JSON helpers backed by first-party implementations.
pub mod json {
    use super::{Error, Result};

    pub fn to_string<T>(_value: &T) -> Result<String> {
        Err(Error::unimplemented("json::to_string"))
    }

    pub fn from_str<T>(_input: &str) -> Result<T> {
        Err(Error::unimplemented("json::from_str"))
    }

    pub fn to_vec<T>(_value: &T) -> Result<Vec<u8>> {
        Err(Error::unimplemented("json::to_vec"))
    }

    pub fn from_slice<T>(_input: &[u8]) -> Result<T> {
        Err(Error::unimplemented("json::from_slice"))
    }
}

/// CBOR helpers routed through the first-party implementation.
pub mod cbor {
    use super::{Error, Result};

    pub fn to_vec<T>(_value: &T) -> Result<Vec<u8>> {
        Err(Error::unimplemented("cbor::to_vec"))
    }

    pub fn from_slice<T>(_input: &[u8]) -> Result<T> {
        Err(Error::unimplemented("cbor::from_slice"))
    }
}

/// Binary helpers covering legacy bincode-like flows.
pub mod binary {
    use super::{Error, Result};

    pub fn encode<T>(_value: &T) -> Result<Vec<u8>> {
        Err(Error::unimplemented("binary::encode"))
    }

    pub fn decode<T>(_input: &[u8]) -> Result<T> {
        Err(Error::unimplemented("binary::decode"))
    }
}

/// TOML helpers for configuration parsing.
pub mod toml {
    use super::{Error, Result};

    pub fn to_string<T>(_value: &T) -> Result<String> {
        Err(Error::unimplemented("toml::to_string"))
    }

    pub fn from_str<T>(_value: &str) -> Result<T> {
        Err(Error::unimplemented("toml::from_str"))
    }
}

/// Convenience macro to highlight unimplemented serialization paths during development.
#[macro_export]
macro_rules! todo_serialization {
    ($component:expr $(,)?) => {
        return Err($crate::Error::unimplemented($component));
    };
}
