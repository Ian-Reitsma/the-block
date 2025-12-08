//! Deserialization traits.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod value;

/// A **data structure** that can be deserialized from any data format supported
/// by Serde.
pub trait Deserialize<'de>: Sized {
    /// Deserialize this value from the given Serde deserializer.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>;
}

/// A data structure that can be deserialized without borrowing any data from
/// the deserializer.
///
/// This is primarily useful as a bound on `Deserialize` types that can be
/// deserialized from any lifetime.
pub trait DeserializeOwned: for<'de> Deserialize<'de> {}

impl<T> DeserializeOwned for T where T: for<'de> Deserialize<'de> {}

/// `DeserializeSeed` is the stateful form of the `Deserialize` trait. If you
/// ever find yourself looking for a way to pass data into a `Deserialize` impl,
/// this trait is the way to do it.
pub trait DeserializeSeed<'de>: Sized {
    /// The type produced by using this seed.
    type Value;

    /// Equivalent to the more common `Deserialize::deserialize` method, except
    /// with some initial piece of data (the seed) passed in.
    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>;
}

impl<'de, T> DeserializeSeed<'de> for std::marker::PhantomData<T>
where
    T: Deserialize<'de>,
{
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer)
    }
}

/// A **data format** that can deserialize any data structure supported by
/// Serde.
pub trait Deserializer<'de>: Sized {
    /// The error type that can be returned if some error occurs during
    /// deserialization.
    type Error: Error;

    /// Require the `Deserializer` to figure out how to drive the visitor based
    /// on what data type is in the input.
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `bool` value.
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `i8` value.
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `i16` value.
    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `i32` value.
    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `i64` value.
    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `i128` value.
    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `u8` value.
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `u16` value.
    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `u32` value.
    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `u64` value.
    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `u128` value.
    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `usize` value.
    fn deserialize_usize<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `isize` value.
    fn deserialize_isize<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `f32` value.
    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an `f64` value.
    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a `char` value.
    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a string value.
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a string value.
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a byte array.
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a byte buffer.
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an optional value.
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a unit value.
    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a unit struct with a
    /// particular name.
    fn deserialize_unit_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a newtype struct with a
    /// particular name.
    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a sequence of values.
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a sequence of values and
    /// knows how many values there are without looking at the serialized data.
    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a tuple struct with a
    /// particular name and number of fields.
    fn deserialize_tuple_struct<V>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a map of key-value pairs.
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting a struct with a particular
    /// name and fields.
    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting an enum value with a
    /// particular name and possible variants.
    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type needs to deserialize a value whose type
    /// doesn't matter because it is ignored.
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Hint that the `Deserialize` type is expecting the name of a struct
    /// field or the discriminant of an enum variant.
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    /// Determine whether `Deserialize` implementations should expect to
    /// deserialize their human-readable form.
    fn is_human_readable(&self) -> bool {
        false
    }
}

/// This trait represents a visitor that walks through a deserializer.
pub trait Visitor<'de>: Sized {
    /// The value produced by this visitor.
    type Value;

    /// Format a message stating what data this Visitor expects to receive.
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result;

    /// The input contains a boolean.
    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Bool(v), &self))
    }

    /// The input contains an `i8`.
    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_i64(i64::from(v))
    }

    /// The input contains an `i16`.
    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_i64(i64::from(v))
    }

    /// The input contains an `i32`.
    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_i64(i64::from(v))
    }

    /// The input contains an `i64`.
    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Signed(v), &self))
    }

    /// The input contains an `i128`.
    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let _ = v;
        Err(Error::invalid_type(Unexpected::Other("i128"), &self))
    }

    /// The input contains a `u8`.
    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_u64(u64::from(v))
    }

    /// The input contains a `u16`.
    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_u64(u64::from(v))
    }

    /// The input contains a `u32`.
    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_u64(u64::from(v))
    }

    /// The input contains a `u64`.
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Unsigned(v), &self))
    }

    /// The input contains a `u128`.
    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let _ = v;
        Err(Error::invalid_type(Unexpected::Other("u128"), &self))
    }

    /// The input contains a `usize`.
    fn visit_usize<E>(self, v: usize) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_u64(v as u64)
    }

    /// The input contains an `isize`.
    fn visit_isize<E>(self, v: isize) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_i64(v as i64)
    }

    /// The input contains an `f32`.
    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_f64(f64::from(v))
    }

    /// The input contains an `f64`.
    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Float(v), &self))
    }

    /// The input contains a `char`.
    fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(v.encode_utf8(&mut [0u8; 4]))
    }

    /// The input contains a string.
    fn visit_str<E>(self, _v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Str, &self))
    }

    /// The input contains a string that lives at least as long as the
    /// `Deserializer`.
    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(v)
    }

    /// The input contains a string and ownership of the string is being given
    /// to the `Visitor`.
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(&v)
    }

    /// The input contains a byte array.
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let _ = v;
        Err(Error::invalid_type(Unexpected::Bytes, &self))
    }

    /// The input contains a byte array that lives at least as long as the
    /// `Deserializer`.
    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_bytes(v)
    }

    /// The input contains a byte array and ownership of the byte array is being
    /// given to the `Visitor`.
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_bytes(&v)
    }

    /// The input contains an optional that is absent.
    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Option, &self))
    }

    /// The input contains an optional that is present.
    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = deserializer;
        Err(Error::invalid_type(Unexpected::Option, &self))
    }

    /// The input contains a unit `()`.
    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: Error,
    {
        Err(Error::invalid_type(Unexpected::Unit, &self))
    }

    /// The input contains a newtype struct.
    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _ = deserializer;
        Err(Error::invalid_type(Unexpected::NewtypeStruct, &self))
    }

    /// The input contains a sequence of elements.
    fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let _ = seq;
        Err(Error::invalid_type(Unexpected::Seq, &self))
    }

    /// The input contains a key-value map.
    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let _ = map;
        Err(Error::invalid_type(Unexpected::Map, &self))
    }

    /// The input contains an enum.
    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        let _ = data;
        Err(Error::invalid_type(Unexpected::Enum, &self))
    }
}

/// Provides a `Visitor` access to each element of a sequence in the input.
pub trait SeqAccess<'de> {
    /// The error type that can be returned if some error occurs during
    /// deserialization.
    type Error: Error;

    /// This returns `Ok(Some(value))` for the next value in the sequence, or
    /// `Ok(None)` if there are no more remaining items.
    fn next_element<T>(&mut self) -> Result<Option<T>, Self::Error>
    where
        T: Deserialize<'de>,
    {
        self.next_element_seed(std::marker::PhantomData)
    }

    /// This returns `Ok(Some(value))` for the next value in the sequence using
    /// the provided seed, or `Ok(None)` if there are no more remaining items.
    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>;

    /// Returns the number of elements remaining in the sequence, if known.
    fn size_hint(&self) -> Option<usize> {
        None
    }
}

/// Provides a `Visitor` access to each entry of a map in the input.
pub trait MapAccess<'de> {
    /// The error type that can be returned if some error occurs during
    /// deserialization.
    type Error: Error;

    /// This returns `Ok(Some(key))` for the next key in the map, or `Ok(None)`
    /// if there are no more remaining entries.
    fn next_key<K>(&mut self) -> Result<Option<K>, Self::Error>
    where
        K: Deserialize<'de>,
    {
        self.next_key_seed(std::marker::PhantomData)
    }

    /// This returns `Ok(Some(key))` for the next key in the map using the
    /// provided seed, or `Ok(None)` if there are no more remaining entries.
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>;

    /// This returns `Ok(value)` for the next value in the map.
    fn next_value<V>(&mut self) -> Result<V, Self::Error>
    where
        V: Deserialize<'de>,
    {
        self.next_value_seed(std::marker::PhantomData)
    }

    /// This returns `Ok(value)` for the next value in the map using the
    /// provided seed.
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>;

    /// This returns `Ok(Some((key, value)))` for the next (key-value) pair in
    /// the map, or `Ok(None)` if there are no more remaining items.
    fn next_entry<K, V>(&mut self) -> Result<Option<(K, V)>, Self::Error>
    where
        K: Deserialize<'de>,
        V: Deserialize<'de>,
    {
        match self.next_key()? {
            Some(key) => {
                let value = self.next_value()?;
                Ok(Some((key, value)))
            }
            None => Ok(None),
        }
    }

    /// This returns `Ok(Some((key, value)))` for the next (key-value) pair in
    /// the map using the provided seeds, or `Ok(None)` if there are no more
    /// remaining items.
    fn next_entry_seed<K, V>(
        &mut self,
        kseed: K,
        vseed: V,
    ) -> Result<Option<(K::Value, V::Value)>, Self::Error>
    where
        K: DeserializeSeed<'de>,
        V: DeserializeSeed<'de>,
    {
        match self.next_key_seed(kseed)? {
            Some(key) => {
                let value = self.next_value_seed(vseed)?;
                Ok(Some((key, value)))
            }
            None => Ok(None),
        }
    }

    /// Returns the number of entries remaining in the map, if known.
    fn size_hint(&self) -> Option<usize> {
        None
    }
}

/// Provides a `Visitor` access to the data of an enum in the input.
pub trait EnumAccess<'de>: Sized {
    /// The error type that can be returned if some error occurs during
    /// deserialization.
    type Error: Error;

    /// The `Visitor` for deserializing the content of the enum variant.
    type Variant: VariantAccess<'de, Error = Self::Error>;

    /// `variant` is called to identify which variant to deserialize.
    fn variant<V>(self) -> Result<(V, Self::Variant), Self::Error>
    where
        V: Deserialize<'de>,
    {
        self.variant_seed(std::marker::PhantomData)
    }

    /// `variant_seed` is called to identify which variant to deserialize using
    /// a seed.
    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>;
}

/// Provides a `Visitor` access to the content of an enum variant in the input.
pub trait VariantAccess<'de>: Sized {
    /// The error type that can be returned if some error occurs during
    /// deserialization.
    type Error: Error;

    /// Called when deserializing a variant with no values.
    fn unit_variant(self) -> Result<(), Self::Error>;

    /// Called when deserializing a tuple-like variant.
    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Called when deserializing a struct-like variant.
    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>;

    /// Called when deserializing a newtype variant.
    fn newtype_variant<T>(self) -> Result<T, Self::Error>
    where
        T: Deserialize<'de>;
}

/// Trait used by `Deserialize` implementations to generically construct errors
/// belonging to the `Deserializer` against which they are currently running.
pub trait Error: Sized + fmt::Debug + fmt::Display {
    /// Used when a [`Deserialize`] receives a type different from what it was
    /// expecting.
    fn custom<T>(msg: T) -> Self
    where
        T: Display;

    /// Raised when a `Deserialize` receives a value of the right type but that
    /// is wrong for some other reason.
    fn invalid_value(unexp: Unexpected, exp: &dyn Expected) -> Self {
        Error::custom(format_args!("invalid value: {}, expected {}", unexp, exp))
    }

    /// Raised when deserializing a sequence or map and the input data contains
    /// too many or too few elements.
    fn invalid_length(len: usize, exp: &dyn Expected) -> Self {
        Error::custom(format_args!("invalid length {}, expected {}", len, exp))
    }

    /// Raised when a [`Deserialize`] receives an unexpected variant of some
    /// type.
    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
        if expected.is_empty() {
            Error::custom(format_args!("unknown variant `{}`", variant))
        } else {
            Error::custom(format_args!(
                "unknown variant `{}`, expected one of: {}",
                variant,
                expected.join(", ")
            ))
        }
    }

    /// Raised when a [`Deserialize`] receives a field with an unrecognized
    /// name.
    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        if expected.is_empty() {
            Error::custom(format_args!("unknown field `{}`", field))
        } else {
            Error::custom(format_args!(
                "unknown field `{}`, expected one of: {}",
                field,
                expected.join(", ")
            ))
        }
    }

    /// Raised when a [`Deserialize`] expects to receive a required field with a
    /// particular name but that field was not present in the input.
    fn missing_field(field: &'static str) -> Self {
        Error::custom(format_args!("missing field `{}`", field))
    }

    /// Raised when a [`Deserialize`] receives more than one of the same field.
    fn duplicate_field(field: &'static str) -> Self {
        Error::custom(format_args!("duplicate field `{}`", field))
    }

    /// Raised when a `Deserialize` receives a type different from what it was
    /// expecting.
    fn invalid_type(unexp: Unexpected, exp: &dyn Expected) -> Self {
        Error::custom(format_args!("invalid type: {}, expected {}", unexp, exp))
    }
}

impl Error for fmt::Error {
    fn custom<T>(_msg: T) -> Self
    where
        T: Display,
    {
        fmt::Error
    }
}

/// `Unexpected` represents an unexpected invocation of any one of the `Visitor`
/// trait methods.
#[derive(Debug)]
pub enum Unexpected<'a> {
    Bool(bool),
    Unsigned(u64),
    Signed(i64),
    Float(f64),
    Char(char),
    Str,
    Bytes,
    Unit,
    Option,
    NewtypeStruct,
    Seq,
    Map,
    Enum,
    UnitVariant,
    NewtypeVariant,
    TupleVariant,
    StructVariant,
    Other(&'a str),
}

impl<'a> Display for Unexpected<'a> {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Unexpected::Bool(b) => write!(formatter, "boolean `{}`", b),
            Unexpected::Unsigned(i) => write!(formatter, "integer `{}`", i),
            Unexpected::Signed(i) => write!(formatter, "integer `{}`", i),
            Unexpected::Float(f) => write!(formatter, "floating point `{}`", f),
            Unexpected::Char(c) => write!(formatter, "character `{}`", c),
            Unexpected::Str => write!(formatter, "string"),
            Unexpected::Bytes => write!(formatter, "byte array"),
            Unexpected::Unit => write!(formatter, "unit value"),
            Unexpected::Option => write!(formatter, "Option value"),
            Unexpected::NewtypeStruct => write!(formatter, "newtype struct"),
            Unexpected::Seq => write!(formatter, "sequence"),
            Unexpected::Map => write!(formatter, "map"),
            Unexpected::Enum => write!(formatter, "enum"),
            Unexpected::UnitVariant => write!(formatter, "unit variant"),
            Unexpected::NewtypeVariant => write!(formatter, "newtype variant"),
            Unexpected::TupleVariant => write!(formatter, "tuple variant"),
            Unexpected::StructVariant => write!(formatter, "struct variant"),
            Unexpected::Other(s) => write!(formatter, "{}", s),
        }
    }
}

/// `Expected` represents an explanation of what data a `Visitor` was expecting
/// to receive.
pub trait Expected {
    /// Format an explanation of what data was being expected. Same signature as
    /// the `Display` and `Debug` traits.
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result;
}

impl Expected for &str {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self)
    }
}

impl Expected for String {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self)
    }
}

impl Expected for &String {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self)
    }
}

impl<'de, T> Expected for T
where
    T: Visitor<'de>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.expecting(formatter)
    }
}

impl Display for dyn Expected + '_ {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        Expected::fmt(self, formatter)
    }
}

// ========================================================================
// Implementations of Deserialize for primitive and standard library types
// ========================================================================

macro_rules! impl_deserialize_num {
    ($ty:ty, $method:ident, $visit:ident) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct NumVisitor;

                impl<'de> Visitor<'de> for NumVisitor {
                    type Value = $ty;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str(concat!("a ", stringify!($ty)))
                    }

                    fn $visit<E>(self, value: $ty) -> Result<$ty, E>
                    where
                        E: Error,
                    {
                        Ok(value)
                    }
                }

                deserializer.$method(NumVisitor)
            }
        }
    };
}

impl_deserialize_num!(bool, deserialize_bool, visit_bool);
impl_deserialize_num!(i8, deserialize_i8, visit_i8);
impl_deserialize_num!(i16, deserialize_i16, visit_i16);
impl_deserialize_num!(i32, deserialize_i32, visit_i32);
impl_deserialize_num!(i64, deserialize_i64, visit_i64);
impl_deserialize_num!(i128, deserialize_i128, visit_i128);
impl_deserialize_num!(u8, deserialize_u8, visit_u8);
impl_deserialize_num!(u16, deserialize_u16, visit_u16);
impl_deserialize_num!(u32, deserialize_u32, visit_u32);
impl_deserialize_num!(u64, deserialize_u64, visit_u64);
impl_deserialize_num!(u128, deserialize_u128, visit_u128);
impl_deserialize_num!(usize, deserialize_usize, visit_usize);
impl_deserialize_num!(isize, deserialize_isize, visit_isize);
impl_deserialize_num!(f32, deserialize_f32, visit_f32);
impl_deserialize_num!(f64, deserialize_f64, visit_f64);

impl<'de> Deserialize<'de> for char {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CharVisitor;

        impl<'de> Visitor<'de> for CharVisitor {
            type Value = char;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a character")
            }

            fn visit_char<E>(self, value: char) -> Result<char, E>
            where
                E: Error,
            {
                Ok(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<char, E>
            where
                E: Error,
            {
                let mut chars = value.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Ok(c),
                    _ => Err(Error::invalid_value(Unexpected::Str, &self)),
                }
            }
        }

        deserializer.deserialize_char(CharVisitor)
    }
}

impl<'de> Deserialize<'de> for &'de str {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrVisitor;

        impl<'de> Visitor<'de> for StrVisitor {
            type Value = &'de str;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a borrowed string")
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<&'de str, E>
            where
                E: Error,
            {
                Ok(value)
            }

            fn visit_str<E>(self, _value: &str) -> Result<&'de str, E>
            where
                E: Error,
            {
                Err(Error::invalid_type(Unexpected::Str, &self))
            }
        }

        deserializer.deserialize_str(StrVisitor)
    }
}

impl<'de> Deserialize<'de> for String {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StringVisitor;

        impl<'de> Visitor<'de> for StringVisitor {
            type Value = String;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, value: &str) -> Result<String, E>
            where
                E: Error,
            {
                Ok(value.to_owned())
            }

            fn visit_string<E>(self, value: String) -> Result<String, E>
            where
                E: Error,
            {
                Ok(value)
            }
        }

        deserializer.deserialize_string(StringVisitor)
    }
}

impl<'de> Deserialize<'de> for Cow<'de, str> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CowVisitor;

        impl<'de> Visitor<'de> for CowVisitor {
            type Value = Cow<'de, str>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Cow<'de, str>, E>
            where
                E: Error,
            {
                Ok(Cow::Borrowed(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Cow<'de, str>, E>
            where
                E: Error,
            {
                Ok(Cow::Owned(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Cow<'de, str>, E>
            where
                E: Error,
            {
                Ok(Cow::Owned(value))
            }
        }

        deserializer.deserialize_string(CowVisitor)
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug)]
        enum Field {
            Secs,
            Nanos,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("`secs` or `nanos`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: Error,
                    {
                        match value {
                            "secs" => Ok(Field::Secs),
                            "nanos" => Ok(Field::Nanos),
                            _ => Err(Error::unknown_field(value, &["secs", "nanos"])),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct DurationVisitor;

        impl<'de> Visitor<'de> for DurationVisitor {
            type Value = Duration;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Duration")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Duration, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut secs = None;
                let mut nanos = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Secs => {
                            if secs.is_some() {
                                return Err(Error::duplicate_field("secs"));
                            }
                            secs = Some(map.next_value()?);
                        }
                        Field::Nanos => {
                            if nanos.is_some() {
                                return Err(Error::duplicate_field("nanos"));
                            }
                            nanos = Some(map.next_value()?);
                        }
                    }
                }

                let secs: u64 = secs.ok_or_else(|| Error::missing_field("secs"))?;
                let nanos: u32 = nanos.ok_or_else(|| Error::missing_field("nanos"))?;

                Ok(Duration::new(secs, nanos))
            }
        }

        deserializer.deserialize_struct("Duration", &["secs", "nanos"], DurationVisitor)
    }
}

impl<'de> Deserialize<'de> for Instant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Instant is not truly deserializable across processes.
        // We deserialize the placeholder and return Instant::now()
        // In practice, users should use #[serde(skip)] or custom deserialization.
        #[derive(Debug)]
        enum Field {
            Secs,
            Nanos,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("`secs` or `nanos`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: Error,
                    {
                        match value {
                            "secs" => Ok(Field::Secs),
                            "nanos" => Ok(Field::Nanos),
                            _ => Err(Error::unknown_field(value, &["secs", "nanos"])),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct InstantVisitor;

        impl<'de> Visitor<'de> for InstantVisitor {
            type Value = Instant;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Instant")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Instant, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut _secs = None;
                let mut _nanos = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Secs => {
                            if _secs.is_some() {
                                return Err(Error::duplicate_field("secs"));
                            }
                            _secs = Some(map.next_value::<u64>()?);
                        }
                        Field::Nanos => {
                            if _nanos.is_some() {
                                return Err(Error::duplicate_field("nanos"));
                            }
                            _nanos = Some(map.next_value::<u32>()?);
                        }
                    }
                }

                // Return current instant as placeholder since Instant can't be meaningfully deserialized
                Ok(Instant::now())
            }
        }

        deserializer.deserialize_struct("Instant", &["secs", "nanos"], InstantVisitor)
    }
}

impl<'de> Deserialize<'de> for SystemTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug)]
        enum Field {
            SecsSinceEpoch,
            Nanos,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("`secs_since_epoch` or `nanos`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: Error,
                    {
                        match value {
                            "secs_since_epoch" => Ok(Field::SecsSinceEpoch),
                            "nanos" => Ok(Field::Nanos),
                            _ => Err(Error::unknown_field(value, &["secs_since_epoch", "nanos"])),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct SystemTimeVisitor;

        impl<'de> Visitor<'de> for SystemTimeVisitor {
            type Value = SystemTime;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct SystemTime")
            }

            fn visit_map<A>(self, mut map: A) -> Result<SystemTime, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut secs_since_epoch = None;
                let mut nanos = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SecsSinceEpoch => {
                            if secs_since_epoch.is_some() {
                                return Err(Error::duplicate_field("secs_since_epoch"));
                            }
                            secs_since_epoch = Some(map.next_value()?);
                        }
                        Field::Nanos => {
                            if nanos.is_some() {
                                return Err(Error::duplicate_field("nanos"));
                            }
                            nanos = Some(map.next_value()?);
                        }
                    }
                }

                let secs: u64 = secs_since_epoch.ok_or_else(|| Error::missing_field("secs_since_epoch"))?;
                let nanos: u32 = nanos.ok_or_else(|| Error::missing_field("nanos"))?;

                Ok(UNIX_EPOCH + Duration::new(secs, nanos))
            }
        }

        deserializer.deserialize_struct("SystemTime", &["secs_since_epoch", "nanos"], SystemTimeVisitor)
    }
}

// Network types - deserialize from strings
impl<'de> Deserialize<'de> for Ipv4Addr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Ipv4AddrVisitor;

        impl<'de> Visitor<'de> for Ipv4AddrVisitor {
            type Value = Ipv4Addr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid IPv4 address string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Ipv4Addr, E>
            where
                E: Error,
            {
                value.parse::<Ipv4Addr>().map_err(|_| {
                    Error::custom(format!("invalid IPv4 address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(Ipv4AddrVisitor)
    }
}

impl<'de> Deserialize<'de> for Ipv6Addr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Ipv6AddrVisitor;

        impl<'de> Visitor<'de> for Ipv6AddrVisitor {
            type Value = Ipv6Addr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid IPv6 address string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Ipv6Addr, E>
            where
                E: Error,
            {
                value.parse::<Ipv6Addr>().map_err(|_| {
                    Error::custom(format!("invalid IPv6 address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(Ipv6AddrVisitor)
    }
}

impl<'de> Deserialize<'de> for IpAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IpAddrVisitor;

        impl<'de> Visitor<'de> for IpAddrVisitor {
            type Value = IpAddr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid IP address string (IPv4 or IPv6)")
            }

            fn visit_str<E>(self, value: &str) -> Result<IpAddr, E>
            where
                E: Error,
            {
                value.parse::<IpAddr>().map_err(|_| {
                    Error::custom(format!("invalid IP address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(IpAddrVisitor)
    }
}

impl<'de> Deserialize<'de> for SocketAddrV4 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SocketAddrV4Visitor;

        impl<'de> Visitor<'de> for SocketAddrV4Visitor {
            type Value = SocketAddrV4;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid IPv4 socket address string (e.g., 127.0.0.1:8080)")
            }

            fn visit_str<E>(self, value: &str) -> Result<SocketAddrV4, E>
            where
                E: Error,
            {
                value.parse::<SocketAddrV4>().map_err(|_| {
                    Error::custom(format!("invalid IPv4 socket address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(SocketAddrV4Visitor)
    }
}

impl<'de> Deserialize<'de> for SocketAddrV6 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SocketAddrV6Visitor;

        impl<'de> Visitor<'de> for SocketAddrV6Visitor {
            type Value = SocketAddrV6;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid IPv6 socket address string (e.g., [::1]:8080)")
            }

            fn visit_str<E>(self, value: &str) -> Result<SocketAddrV6, E>
            where
                E: Error,
            {
                value.parse::<SocketAddrV6>().map_err(|_| {
                    Error::custom(format!("invalid IPv6 socket address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(SocketAddrV6Visitor)
    }
}

impl<'de> Deserialize<'de> for SocketAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SocketAddrVisitor;

        impl<'de> Visitor<'de> for SocketAddrVisitor {
            type Value = SocketAddr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a valid socket address string (IPv4 or IPv6)")
            }

            fn visit_str<E>(self, value: &str) -> Result<SocketAddr, E>
            where
                E: Error,
            {
                value.parse::<SocketAddr>().map_err(|_| {
                    Error::custom(format!("invalid socket address: {}", value))
                })
            }
        }

        deserializer.deserialize_str(SocketAddrVisitor)
    }
}

impl<'de, T> Deserialize<'de> for Option<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for OptionVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = Option<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("option")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                T::deserialize(deserializer).map(Some)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(None)
            }
        }

        deserializer.deserialize_option(OptionVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, T> Deserialize<'de> for Vec<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VecVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for VecVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = Vec<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_seq(VecVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, T> Deserialize<'de> for VecDeque<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VecDequeVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for VecDequeVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = VecDeque<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = VecDeque::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(value) = seq.next_element()? {
                    values.push_back(value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_seq(VecDequeVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, K, V> Deserialize<'de> for std::collections::HashMap<K, V>
where
    K: Deserialize<'de> + std::hash::Hash + Eq,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HashMapVisitor<K, V> {
            marker: std::marker::PhantomData<(K, V)>,
        }

        impl<'de, K, V> Visitor<'de> for HashMapVisitor<K, V>
        where
            K: Deserialize<'de> + std::hash::Hash + Eq,
            V: Deserialize<'de>,
        {
            type Value = std::collections::HashMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values =
                    std::collections::HashMap::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, value)) = map.next_entry()? {
                    values.insert(key, value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_map(HashMapVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, K, V> Deserialize<'de> for std::collections::BTreeMap<K, V>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BTreeMapVisitor<K, V> {
            marker: std::marker::PhantomData<(K, V)>,
        }

        impl<'de, K, V> Visitor<'de> for BTreeMapVisitor<K, V>
        where
            K: Deserialize<'de> + Ord,
            V: Deserialize<'de>,
        {
            type Value = std::collections::BTreeMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = std::collections::BTreeMap::new();
                while let Some((key, value)) = map.next_entry()? {
                    values.insert(key, value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_map(BTreeMapVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, T> Deserialize<'de> for std::collections::HashSet<T>
where
    T: Deserialize<'de> + std::hash::Hash + Eq,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HashSetVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for HashSetVisitor<T>
        where
            T: Deserialize<'de> + std::hash::Hash + Eq,
        {
            type Value = std::collections::HashSet<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values =
                    std::collections::HashSet::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(value) = seq.next_element()? {
                    values.insert(value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_seq(HashSetVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de, T> Deserialize<'de> for std::collections::BTreeSet<T>
where
    T: Deserialize<'de> + Ord,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BTreeSetVisitor<T> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for BTreeSetVisitor<T>
        where
            T: Deserialize<'de> + Ord,
        {
            type Value = std::collections::BTreeSet<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = std::collections::BTreeSet::new();
                while let Some(value) = seq.next_element()? {
                    values.insert(value);
                }
                Ok(values)
            }
        }

        deserializer.deserialize_seq(BTreeSetVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

impl<'de> Deserialize<'de> for () {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UnitVisitor;

        impl<'de> Visitor<'de> for UnitVisitor {
            type Value = ();

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("unit")
            }

            fn visit_unit<E>(self) -> Result<(), E>
            where
                E: Error,
            {
                Ok(())
            }
        }

        deserializer.deserialize_unit(UnitVisitor)
    }
}

// Implement for tuples up to size 16
macro_rules! tuple_impls {
    ($($len:tt => ($($n:tt $name:ident)+))+) => {
        $(
            impl<'de, $($name: Deserialize<'de>),+> Deserialize<'de> for ($($name,)+) {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    struct TupleVisitor<$($name,)+> {
                        marker: std::marker::PhantomData<($($name,)+)>,
                    }

                    impl<'de, $($name: Deserialize<'de>),+> Visitor<'de> for TupleVisitor<$($name,)+> {
                        type Value = ($($name,)+);

                        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                            formatter.write_str(concat!("a tuple of size ", stringify!($len)))
                        }

                        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                        where
                            A: SeqAccess<'de>,
                        {
                            Ok(($(
                                match seq.next_element()? {
                                    Some(value) => value,
                                    None => return Err(Error::invalid_length($n, &self)),
                                },
                            )+))
                        }
                    }

                    deserializer.deserialize_tuple($len, TupleVisitor { marker: std::marker::PhantomData })
                }
            }
        )+
    }
}

tuple_impls! {
    1 => (0 T0)
    2 => (0 T0 1 T1)
    3 => (0 T0 1 T1 2 T2)
    4 => (0 T0 1 T1 2 T2 3 T3)
    5 => (0 T0 1 T1 2 T2 3 T3 4 T4)
    6 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5)
    7 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6)
    8 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7)
    9 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8)
    10 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9)
    11 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10)
    12 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11)
    13 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12)
    14 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13)
    15 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14)
    16 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14 15 T15)
}

// Implement for arrays
impl<'de, T, const N: usize> Deserialize<'de> for [T; N]
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ArrayVisitor<T, const N: usize> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T, const N: usize> Visitor<'de> for ArrayVisitor<T, N>
        where
            T: Deserialize<'de>,
        {
            type Value = [T; N];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "an array of length {}", N)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                // Collect into a Vec first, then convert to array
                let mut vec = Vec::with_capacity(N);
                for i in 0..N {
                    match seq.next_element()? {
                        Some(value) => vec.push(value),
                        None => return Err(Error::invalid_length(i, &self)),
                    }
                }
                // Check for trailing elements
                if seq.next_element::<T>()?.is_some() {
                    return Err(Error::invalid_length(N + 1, &self));
                }
                // Convert Vec to array - this is safe because we know vec.len() == N
                vec.try_into()
                    .map_err(|_| Error::custom("array length mismatch"))
            }
        }

        deserializer.deserialize_tuple(N, ArrayVisitor {
            marker: std::marker::PhantomData,
        })
    }
}

/// A type that can be used to deserialize and discard data efficiently.
///
/// This is useful for ignoring unknown fields in formats that require
/// consuming the data.
pub struct IgnoredAny;

impl<'de> Deserialize<'de> for IgnoredAny {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IgnoredAnyVisitor;

        impl<'de> Visitor<'de> for IgnoredAnyVisitor {
            type Value = IgnoredAny;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("any value")
            }

            fn visit_bool<E>(self, _v: bool) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_i64<E>(self, _v: i64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_u64<E>(self, _v: u64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_f64<E>(self, _v: f64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_str<E>(self, _v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_string<E>(self, _v: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                IgnoredAny::deserialize(deserializer)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                while let Some(_) = seq.next_element::<IgnoredAny>()? {}
                Ok(IgnoredAny)
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                while let Some((_, _)) = map.next_entry::<IgnoredAny, IgnoredAny>()? {}
                Ok(IgnoredAny)
            }

            fn visit_bytes<E>(self, _v: &[u8]) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }

            fn visit_byte_buf<E>(self, _v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(IgnoredAny)
            }
        }

        deserializer.deserialize_any(IgnoredAnyVisitor)
    }
}
