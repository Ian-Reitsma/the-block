#![forbid(unsafe_code)]

//! First-party serialization facade used across the workspace.
//!
//! The helpers exposed by this crate intentionally avoid third-party
//! implementations so downstream crates can opt into deterministic, auditable
//! encoders without depending on serde_json, bincode, or toml.  JSON and binary
//! helpers rely on the implementations located in `json.rs` and `binary.rs`
//! respectively while continuing to interoperate with `serde` derives.

mod base58_impl;
mod binary_impl;
mod hex_impl;
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

// Re-export traits from foundation_serde (aliased as 'serde' in Cargo.toml)
pub use ::serde::Deserialize;
pub use ::serde::Serialize;
pub use ::serde::{de, ser};

// Re-export derive macros from foundation_serde_derive
// Note: In Rust, derive macros and traits can share names as they're in different namespaces
pub use foundation_serde_derive::Deserialize;
pub use foundation_serde_derive::Serialize;

pub mod serde {
    // Re-export all foundation_serde items
    pub use ::serde::*;
    // Re-export derive macros
    pub use foundation_serde_derive::{Deserialize, Serialize};
}

pub mod binary_cursor;

/// Helpers that provide serde-style default values without depending on
/// third-party attribute macros.
pub mod defaults {
    /// Return [`Default::default()`] for the requested type.  This mirrors
    /// `#[serde(default)]` behaviour so downstream crates can express defaulted
    /// fields via the facade.
    pub fn default<T: Default>() -> T {
        T::default()
    }

    /// Return `true`.
    pub fn true_() -> bool {
        true
    }

    /// Return `false`.
    pub fn false_() -> bool {
        false
    }
}

/// Helpers that mirror `skip_serializing_if` predicates using first-party
/// functions.  These helpers keep serde derives configured against the facade
/// while avoiding direct references to standard library implementations.
pub mod skip {
    /// Returns `true` when the option is [`None`].
    pub fn option_is_none<T>(value: &Option<T>) -> bool {
        value.is_none()
    }

    /// Generic helper that reports whether the provided collection is empty.
    pub fn is_empty<T: ?Sized + IsEmpty>(value: &T) -> bool {
        value.is_empty()
    }

    /// Trait implemented for collection types that can report emptiness.
    pub trait IsEmpty {
        /// Return `true` when the collection contains no elements.
        fn is_empty(&self) -> bool;
    }

    impl<T> IsEmpty for [T] {
        fn is_empty(&self) -> bool {
            <[T]>::is_empty(self)
        }
    }

    impl<T> IsEmpty for Vec<T> {
        fn is_empty(&self) -> bool {
            Vec::is_empty(self)
        }
    }

    impl<T> IsEmpty for std::collections::VecDeque<T> {
        fn is_empty(&self) -> bool {
            std::collections::VecDeque::is_empty(self)
        }
    }

    impl<K, V, S> IsEmpty for std::collections::HashMap<K, V, S> {
        fn is_empty(&self) -> bool {
            std::collections::HashMap::is_empty(self)
        }
    }

    impl<K, V> IsEmpty for std::collections::BTreeMap<K, V> {
        fn is_empty(&self) -> bool {
            std::collections::BTreeMap::is_empty(self)
        }
    }

    impl<T> IsEmpty for std::collections::BTreeSet<T> {
        fn is_empty(&self) -> bool {
            std::collections::BTreeSet::is_empty(self)
        }
    }

    impl<T, S> IsEmpty for std::collections::HashSet<T, S> {
        fn is_empty(&self) -> bool {
            std::collections::HashSet::is_empty(self)
        }
    }

    impl IsEmpty for String {
        fn is_empty(&self) -> bool {
            String::is_empty(self)
        }
    }

    impl IsEmpty for std::path::Path {
        fn is_empty(&self) -> bool {
            self.as_os_str().is_empty()
        }
    }
}

/// Byte buffer helpers mirroring the external `serde_bytes` crate.
pub mod serde_bytes {
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    /// Serialize any byte-like container using the serializer's native
    /// `serialize_bytes` implementation.
    pub fn serialize<S, T>(bytes: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]> + ?Sized,
    {
        serializer.serialize_bytes(bytes.as_ref())
    }

    /// Deserialize a byte buffer into a `Vec<u8>`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a byte buffer")
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(value.to_vec())
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(value)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(byte) = seq.next_element::<u8>()? {
                    out.push(byte);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

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

    /// Serialize a JSON [`Value`] to a byte vector without requiring serde traits.
    pub fn to_vec_value(value: &Value) -> Vec<u8> {
        json_impl::to_vec_value(value)
    }

    /// Serialize a JSON [`Value`] into a string without requiring serde traits.
    pub fn to_string_value(value: &Value) -> String {
        json_impl::to_string_value(value)
    }

    /// Serialize a JSON [`Value`] into a pretty-printed string without requiring serde traits.
    pub fn to_string_value_pretty(value: &Value) -> String {
        json_impl::to_string_value_pretty(value)
    }

    /// Deserialize a value from a byte slice containing JSON data.
    pub fn from_slice<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
        json_impl::from_slice(input).map_err(Error::json)
    }

    /// Deserialize a JSON [`Value`] from a byte slice.
    pub fn value_from_slice(input: &[u8]) -> Result<Value> {
        json_impl::value_from_slice(input).map_err(Error::json)
    }

    /// Deserialize a JSON [`Value`] from a string slice.
    pub fn value_from_str(input: &str) -> Result<Value> {
        json_impl::value_from_str(input).map_err(Error::json)
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

    /// Convert any serializable value into a JSON [`Value`].
    pub fn from_any<T>(value: T) -> Value
    where
        T: Serialize,
    {
        json_impl::to_value(&value)
            .expect("foundation_serialization::json! value must be serializable")
    }
}

#[doc(hidden)]
pub fn __json_object_key<K>(key: K) -> ::std::string::String
where
    K: ::std::convert::Into<::std::string::String>,
{
    key.into()
}

#[doc(hidden)]
#[macro_export]
macro_rules! json_internal_key {
    ($key:literal) => {
        $key
    };
    ($key:ident) => {
        stringify!($key)
    };
    ($key:expr) => {
        $key
    };
}

#[macro_export]
macro_rules! json {
    (null) => {
        $crate::json::Value::Null
    };
    (true) => {
        $crate::json::Value::Bool(true)
    };
    (false) => {
        $crate::json::Value::Bool(false)
    };
    ([]) => {
        $crate::json::Value::Array(::std::vec::Vec::new())
    };
    ([ $($element:tt),* $(,)? ]) => {
        $crate::json::Value::Array({
            let mut elements: ::std::vec::Vec<$crate::json::Value> = ::std::vec::Vec::new();
            $(
                elements.push($crate::json!($element));
            )*
            elements
        })
    };
    ({}) => {
        $crate::json::Value::Object($crate::json::Map::new())
    };
    ({ $($key:tt : $value:tt),* $(,)? }) => {
        $crate::json::Value::Object({
            let mut object = $crate::json::Map::new();
            $(
                let key = $crate::__json_object_key($crate::json_internal_key!($key));
                object.insert(key, $crate::json!($value));
            )*
            object
        })
    };
    ($other:expr) => {
        $crate::json::from_any($other)
    };
    () => {
        compile_error!("unexpected end of json! macro invocation")
    };
}

/// Base58 helpers implemented without third-party dependencies.
pub mod base58 {
    pub use crate::base58_impl::{decode, encode, Error};
}

/// Hex helpers implemented without third-party dependencies.
pub mod hex {
    pub use crate::hex_impl::{decode, decode_array, encode, Error};
}

/// Binary helpers backed by the first-party encoder/decoder.
pub mod binary {
    use serde::{de::DeserializeOwned, Serialize};

    use super::{binary_impl, Error, Result};

    pub use crate::binary_impl::Error as CodecError;

    /// Serialize a value into an owned byte vector.
    pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
        binary_impl::encode(value).map_err(Error::binary)
    }

    /// Serialize a value directly into the provided buffer.
    pub fn encode_into<T: Serialize>(value: &T, buffer: &mut Vec<u8>) -> Result<()> {
        binary_impl::encode_into(value, buffer).map_err(Error::binary)
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

    pub use super::toml_impl::{parse_table, parse_value, Table, Value};

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
