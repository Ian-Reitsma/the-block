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

pub use serde::{de, ser, Deserialize, Serialize};

pub mod serde {
    pub use serde::*;
}

pub use serde_bytes;

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
#[macro_export]
macro_rules! json_internal {
    // Arrays -----------------------------------------------------------------
    (@array [$($elems:expr,)*]) => {
        $crate::__private::vec![$($elems,)*]
    };

    (@array [$($elems:expr),*]) => {
        $crate::__private::vec![$($elems),*]
    };

    (@array [$($elems:expr,)*] null $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(null)] $($rest)*)
    };

    (@array [$($elems:expr,)*] true $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(true)] $($rest)*)
    };

    (@array [$($elems:expr,)*] false $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(false)] $($rest)*)
    };

    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!([$($array)*])] $($rest)*)
    };

    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!({$($map)*})] $($rest)*)
    };

    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!($next),] $($rest)*)
    };

    (@array [$($elems:expr,)*] $last:expr) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!($last)])
    };

    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)*] $($rest)*)
    };

    (@array [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        $crate::json_unexpected!($unexpected)
    };

    // Objects ----------------------------------------------------------------
    (@object $object:ident () () ()) => {};

    (@object $object:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        let _ = $object.insert(($($key)+).into(), $value);
        $crate::json_internal!(@object $object () ($($rest)*) ($($rest)*));
    };

    (@object $object:ident [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        $crate::json_unexpected!($unexpected);
    };

    (@object $object:ident [$($key:tt)+] ($value:expr)) => {
        let _ = $object.insert(($($key)+).into(), $value);
    };

    (@object $object:ident ($($key:tt)+) (: null $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(null)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: true $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(true)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: false $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(false)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!([$($array)*])) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!({$($map)*})) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!($value)) , $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: $value:expr) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!($value)));
    };

    (@object $object:ident ($($key:tt)+) (:) $copy:tt) => {
        $crate::json_internal!();
    };

    (@object $object:ident ($($key:tt)+) () $copy:tt) => {
        $crate::json_internal!();
    };

    (@object $object:ident () (: $($rest:tt)*) ($colon:tt $($copy:tt)*)) => {
        $crate::json_unexpected!($colon);
    };

    (@object $object:ident ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        $crate::json_unexpected!($comma);
    };

    (@object $object:ident () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object ($key) (: $($rest)*) (: $($rest)*));
    };

    (@object $object:ident ($($key:tt)*) (: $($unexpected:tt)+) $copy:tt) => {
        $crate::json_expect_expr_comma!($($unexpected)+);
    };

    (@object $object:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object ($($key)* $tt) ($($rest)*) ($($rest)*));
    };

    // Main dispatch ----------------------------------------------------------
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
        $crate::json::Value::Array($crate::__private::vec![])
    };

    ([ $($tt:tt)+ ]) => {
        $crate::json::Value::Array($crate::json_internal!(@array [] $($tt)+))
    };

    ({}) => {
        $crate::json::Value::Object($crate::json::Map::new())
    };

    ({ $($tt:tt)+ }) => {
        $crate::json::Value::Object({
            let mut object = $crate::json::Map::new();
            $crate::json_internal!(@object object () ($($tt)+) ($($tt)+));
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

#[macro_export]
macro_rules! json {
    ($($json:tt)+) => {
        $crate::json_internal!($($json)+)
    };

    () => {
        $crate::json_internal!()
    };
}

#[doc(hidden)]
pub mod __private {
    pub use std::vec;
}

#[doc(hidden)]
#[macro_export]
macro_rules! json_internal_vec {
    ($($content:tt)*) => {
        $crate::__private::vec![$($content)*]
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! json_unexpected {
    () => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! json_expect_expr_comma {
    ($e:expr , $($tt:tt)*) => {};
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
