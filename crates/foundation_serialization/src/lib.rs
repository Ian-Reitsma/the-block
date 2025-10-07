#![forbid(unsafe_code)]

use std::fmt;

use ::toml as external_toml;
use external_toml::{de as toml_de, ser as toml_ser};

/// Result alias used by the first-party serialization routines.
pub type Result<T> = std::result::Result<T, Error>;

/// Unified error type covering the supported serialization backends.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Json(serde_json::Error),
    Cbor(serde_cbor::Error),
    Binary(bincode::Error),
    TomlSerialize(toml_ser::Error),
    TomlDeserialize(toml_de::Error),
}

impl Error {
    fn json(err: serde_json::Error) -> Self {
        Self {
            kind: ErrorKind::Json(err),
        }
    }

    fn cbor(err: serde_cbor::Error) -> Self {
        Self {
            kind: ErrorKind::Cbor(err),
        }
    }

    fn binary(err: bincode::Error) -> Self {
        Self {
            kind: ErrorKind::Binary(err),
        }
    }

    fn toml_ser(err: toml_ser::Error) -> Self {
        Self {
            kind: ErrorKind::TomlSerialize(err),
        }
    }

    fn toml_de(err: toml_de::Error) -> Self {
        Self {
            kind: ErrorKind::TomlDeserialize(err),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Json(err) => write!(f, "json serialization failed: {err}"),
            ErrorKind::Cbor(err) => write!(f, "cbor serialization failed: {err}"),
            ErrorKind::Binary(err) => write!(f, "binary serialization failed: {err}"),
            ErrorKind::TomlSerialize(err) => write!(f, "toml serialization failed: {err}"),
            ErrorKind::TomlDeserialize(err) => write!(f, "toml deserialization failed: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Json(err) => Some(err),
            ErrorKind::Cbor(err) => Some(err),
            ErrorKind::Binary(err) => Some(&**err),
            ErrorKind::TomlSerialize(err) => Some(err),
            ErrorKind::TomlDeserialize(err) => Some(err),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::json(value)
    }
}

impl From<serde_cbor::Error> for Error {
    fn from(value: serde_cbor::Error) -> Self {
        Self::cbor(value)
    }
}

impl From<bincode::Error> for Error {
    fn from(value: bincode::Error) -> Self {
        Self::binary(value)
    }
}

impl From<toml_ser::Error> for Error {
    fn from(value: toml_ser::Error) -> Self {
        Self::toml_ser(value)
    }
}

impl From<toml_de::Error> for Error {
    fn from(value: toml_de::Error) -> Self {
        Self::toml_de(value)
    }
}

pub use serde::{de, ser, Deserialize, Serialize};

pub mod serde {
    pub use serde::*;
}
pub use serde_bytes;

/// JSON helpers backed by the first-party implementation.
pub mod json {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::{Error, Result};

    pub use serde_json::{json, Map, Number, Value};

    pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
        serde_json::to_string(value).map_err(Error::from)
    }

    pub fn to_string_pretty<T: Serialize>(value: &T) -> Result<String> {
        serde_json::to_string_pretty(value).map_err(Error::from)
    }

    pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
        serde_json::from_str(input).map_err(Error::from)
    }

    pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        serde_json::to_vec(value).map_err(Error::from)
    }

    pub fn to_vec_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(value).map_err(Error::from)
    }

    pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        serde_json::from_slice(input).map_err(Error::from)
    }

    pub fn to_value<T: Serialize>(value: T) -> Result<Value> {
        serde_json::to_value(value).map_err(Error::from)
    }

    pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
        serde_json::from_value(value).map_err(Error::from)
    }

    pub fn from_reader<R, T>(reader: R) -> Result<T>
    where
        R: std::io::Read,
        T: DeserializeOwned,
    {
        serde_json::from_reader(reader).map_err(Error::from)
    }

    pub fn to_writer_pretty<W, T>(writer: W, value: &T) -> Result<()>
    where
        W: std::io::Write,
        T: Serialize,
    {
        serde_json::to_writer_pretty(writer, value).map_err(Error::from)
    }
}

/// CBOR helpers routed through the first-party implementation.
pub mod cbor {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::{Error, Result};

    pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        serde_cbor::to_vec(value).map_err(Error::from)
    }

    pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        serde_cbor::from_slice(input).map_err(Error::from)
    }
}

/// Binary helpers covering legacy bincode-like flows.
pub mod binary {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::{Error, Result};

    pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        bincode::serialize(value).map_err(Error::from)
    }

    pub fn decode<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        bincode::deserialize(input).map_err(Error::from)
    }
}

/// TOML helpers for configuration parsing.
pub mod toml {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::{external_toml, Error, Result};

    pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
        external_toml::to_string(value).map_err(Error::from)
    }

    pub fn to_string_pretty<T: Serialize>(value: &T) -> Result<String> {
        external_toml::to_string_pretty(value).map_err(Error::from)
    }

    pub fn from_str<T: DeserializeOwned>(value: &str) -> Result<T> {
        external_toml::from_str(value).map_err(Error::from)
    }
}
