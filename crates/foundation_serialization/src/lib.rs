#![forbid(unsafe_code)]

//! First-party serialization facade used across the workspace.
//!
//! The helpers exposed by this crate intentionally avoid third-party
//! implementations so downstream crates can opt into deterministic, auditable
//! encoders without depending on serde_json, bincode, or toml.  JSON and binary
//! helpers rely on the implementations located in `json.rs` and `binary.rs`
//! respectively while continuing to interoperate with `serde` derives.

mod binary_impl;
mod json_impl;
mod toml_impl;

/// Result alias shared by the serialization helpers.
pub type Result<T> = std::result::Result<T, Error>;

/// Unified error returned by the serialization facade.
#[derive(Debug)]
pub enum Error {
    /// JSON serialization failure.
    Json(json_impl::Error),
    /// Binary serialization failure.
    Binary(binary_impl::Error),
    /// TOML serialization failure.
    Toml(toml_impl::Error),
}

impl Error {
    fn json(err: json_impl::Error) -> Self {
        Self::Json(err)
    }

    fn binary(err: binary_impl::Error) -> Self {
        Self::Binary(err)
    }

    fn toml(err: toml_impl::Error) -> Self {
        Self::Toml(err)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Json(err) => err.fmt(f),
            Error::Binary(err) => err.fmt(f),
            Error::Toml(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Json(err) => Some(err),
            Error::Binary(err) => Some(err),
            Error::Toml(err) => Some(err),
        }
    }
}

pub use serde::{de, ser, Deserialize, Serialize};

pub mod serde {
    pub use serde::*;
}

pub use serde_bytes;

/// JSON helpers backed by the first-party encoder/decoder.
pub mod json {
    use std::io::{Read, Write};

    use serde::{de::DeserializeOwned, Serialize};

    use super::{json_impl, Error, Result};

    pub use crate::json_impl::{Map, Number, Value};

    /// Serialize a value into a compact JSON string.
    pub fn to_string<T: Serialize>(value: &T) -> Result<String> {
        json_impl::to_string(value).map_err(Error::json)
    }

    /// Serialize a value into a pretty-printed JSON string.
    pub fn to_string_pretty<T: Serialize>(value: &T) -> Result<String> {
        json_impl::to_string_pretty(value).map_err(Error::json)
    }

    /// Deserialize a value from a JSON string slice.
    pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
        json_impl::from_str(input).map_err(Error::json)
    }

    /// Serialize a value to a byte vector.
    pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        json_impl::to_vec(value).map_err(Error::json)
    }

    /// Serialize a value to a pretty-printed byte vector.
    pub fn to_vec_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        json_impl::to_vec_pretty(value).map_err(Error::json)
    }

    /// Deserialize a value from a byte slice containing JSON data.
    pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        json_impl::from_slice(input).map_err(Error::json)
    }

    /// Deserialize a value from a reader containing JSON text.
    pub fn from_reader<R, T>(mut reader: R) -> Result<T>
    where
        R: Read,
        T: DeserializeOwned,
    {
        let mut buffer = String::new();
        reader
            .read_to_string(&mut buffer)
            .map_err(|err| Error::json(json_impl::Error::io(err)))?;
        json_impl::from_str(&buffer).map_err(Error::json)
    }

    /// Serialize a value into a pretty JSON representation and write it to the
    /// provided writer.
    pub fn to_writer_pretty<W, T>(mut writer: W, value: &T) -> Result<()>
    where
        W: Write,
        T: Serialize,
    {
        let rendered = json_impl::to_string_pretty(value).map_err(Error::json)?;
        writer
            .write_all(rendered.as_bytes())
            .map_err(|err| Error::json(json_impl::Error::io(err)))
    }

    /// Convert a serializable value into a JSON [`Value`].
    pub fn to_value<T: Serialize>(value: T) -> Result<Value> {
        json_impl::to_value(&value).map_err(Error::json)
    }

    /// Deserialize a JSON [`Value`] into a strongly typed structure.
    pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
        json_impl::from_value(value).map_err(Error::json)
    }
}

/// Binary helpers backed by the first-party encoder/decoder.
pub mod binary {
    use serde::{de::DeserializeOwned, Serialize};

    use super::{binary_impl, Error, Result};

    /// Serialize a value into an owned byte vector.
    pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        binary_impl::encode(value).map_err(Error::binary)
    }

    /// Deserialize a value from the provided binary slice.
    pub fn decode<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        binary_impl::decode(input).map_err(Error::binary)
    }
}

/// TOML helpers backed by the first-party encoder/decoder.
pub mod toml {
    use serde::{de::DeserializeOwned, Serialize};

    use super::{toml_impl, Error, Result};

    /// Deserialize a value from a TOML string slice.
    pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T> {
        toml_impl::from_str(input).map_err(Error::toml)
    }

    /// Serialize a value into a compact TOML string.
    pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String> {
        toml_impl::to_string(value).map_err(Error::toml)
    }

    /// Serialize a value into a pretty TOML string with section headers grouped.
    pub fn to_string_pretty<T: Serialize + ?Sized>(value: &T) -> Result<String> {
        toml_impl::to_string_pretty(value).map_err(Error::toml)
    }

    /// Serialize a value into a byte vector containing TOML text.
    pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
        toml_impl::to_vec(value).map_err(Error::toml)
    }
}
