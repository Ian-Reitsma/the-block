#![allow(dead_code)]

use core::fmt;

#[cfg(feature = "stub-backend")]
pub use foundation_serde_derive::{Deserialize, Serialize};

pub mod ser {
    use super::StubError;
    use core::fmt;

    pub trait Serialize {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer;
    }

    pub trait Serializer: Sized {
        type Ok;
        type Error: Error;

        type SerializeSeq: SerializeSeq<Ok = Self::Ok, Error = Self::Error>;
        type SerializeTuple: SerializeTuple<Ok = Self::Ok, Error = Self::Error>;
        type SerializeTupleStruct: SerializeTupleStruct<Ok = Self::Ok, Error = Self::Error>;
        type SerializeTupleVariant: SerializeTupleVariant<Ok = Self::Ok, Error = Self::Error>;
        type SerializeMap: SerializeMap<Ok = Self::Ok, Error = Self::Error>;
        type SerializeStruct: SerializeStruct<Ok = Self::Ok, Error = Self::Error>;
        type SerializeStructVariant: SerializeStructVariant<Ok = Self::Ok, Error = Self::Error>;

        fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_bool"))
        }

        fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_i8"))
        }

        fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_i16"))
        }

        fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_i32"))
        }

        fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_i64"))
        }

        fn serialize_i128(self, _v: i128) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_i128"))
        }

        fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_u8"))
        }

        fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_u16"))
        }

        fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_u32"))
        }

        fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_u64"))
        }

        fn serialize_u128(self, _v: u128) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_u128"))
        }

        fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_f32"))
        }

        fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_f64"))
        }

        fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_char"))
        }

        fn serialize_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_str"))
        }

        fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_bytes"))
        }

        fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_none"))
        }

        fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
            value.serialize(self)
        }

        fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_unit"))
        }

        fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_unit_struct"))
        }

        fn serialize_unit_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
        ) -> Result<Self::Ok, Self::Error> {
            Err(Self::Error::unsupported("serialize_unit_variant"))
        }

        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _name: &'static str,
            value: &T,
        ) -> Result<Self::Ok, Self::Error> {
            value.serialize(self)
        }

        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            value: &T,
        ) -> Result<Self::Ok, Self::Error> {
            value.serialize(self)
        }

        fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
            Err(Self::Error::unsupported("serialize_seq"))
        }

        fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
            Err(Self::Error::unsupported("serialize_tuple"))
        }

        fn serialize_tuple_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeTupleStruct, Self::Error> {
            Err(Self::Error::unsupported("serialize_tuple_struct"))
        }

        fn serialize_tuple_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeTupleVariant, Self::Error> {
            Err(Self::Error::unsupported("serialize_tuple_variant"))
        }

        fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
            Err(Self::Error::unsupported("serialize_map"))
        }

        fn serialize_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStruct, Self::Error> {
            Err(Self::Error::unsupported("serialize_struct"))
        }

        fn serialize_struct_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStructVariant, Self::Error> {
            Err(Self::Error::unsupported("serialize_struct_variant"))
        }

        fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
            let owned = value.to_string();
            self.serialize_str(&owned)
        }

        fn is_human_readable(&self) -> bool {
            true
        }
    }

    pub trait SerializeSeq {
        type Ok;
        type Error: Error;

        fn serialize_element<T: ?Sized + Serialize>(
            &mut self,
            value: &T,
        ) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_element"))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_seq_end"))
        }
    }

    pub trait SerializeTuple {
        type Ok;
        type Error: Error;

        fn serialize_element<T: ?Sized + Serialize>(
            &mut self,
            value: &T,
        ) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_tuple_element"))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_tuple_end"))
        }
    }

    pub trait SerializeTupleStruct {
        type Ok;
        type Error: Error;

        fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_tuple_struct_field"))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_tuple_struct_end"))
        }
    }

    pub trait SerializeTupleVariant {
        type Ok;
        type Error: Error;

        fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_tuple_variant_field"))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_tuple_variant_end"))
        }
    }

    pub trait SerializeMap {
        type Ok;
        type Error: Error;

        fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
            let _ = key;
            Err(Self::Error::unsupported("serialize_map_key"))
        }

        fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_map_value"))
        }

        fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
            &mut self,
            key: &K,
            value: &V,
        ) -> Result<(), Self::Error> {
            self.serialize_key(key)?;
            self.serialize_value(value)
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_map_end"))
        }
    }

    pub trait SerializeStruct {
        type Ok;
        type Error: Error;

        fn serialize_field<T: ?Sized + Serialize>(
            &mut self,
            _key: &'static str,
            value: &T,
        ) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_struct_field"))
        }

        fn skip_field(&mut self, _key: &'static str) -> Result<(), Self::Error> {
            Err(Self::Error::unsupported("serialize_struct_skip_field"))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_struct_end"))
        }
    }

    pub trait SerializeStructVariant {
        type Ok;
        type Error: Error;

        fn serialize_field<T: ?Sized + Serialize>(
            &mut self,
            _key: &'static str,
            value: &T,
        ) -> Result<(), Self::Error> {
            let _ = value;
            Err(Self::Error::unsupported("serialize_struct_variant_field"))
        }

        fn skip_field(&mut self, _key: &'static str) -> Result<(), Self::Error> {
            Err(Self::Error::unsupported(
                "serialize_struct_variant_skip_field",
            ))
        }

        fn end(self) -> Result<Self::Ok, Self::Error>
        where
            Self: Sized,
        {
            Err(Self::Error::unsupported("serialize_struct_variant_end"))
        }
    }

    pub trait Error: std::error::Error {
        fn custom<T: fmt::Display>(msg: T) -> Self;

        fn unsupported(op: &'static str) -> Self
        where
            Self: Sized,
        {
            Self::custom(format_args!("foundation_serde stub cannot {op}").to_string())
        }
    }

    impl Error for StubError {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            StubError::new(msg)
        }
    }

    macro_rules! unsupported_serialize {
        ($($ty:ty => $op:literal),* $(,)?) => {
            $(impl Serialize for $ty {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    let _ = serializer;
                    Err(S::Error::unsupported($op))
                }
            })*
        };
    }

    unsupported_serialize! {
        bool => "serialize_bool",
        i8 => "serialize_i8",
        i16 => "serialize_i16",
        i32 => "serialize_i32",
        i64 => "serialize_i64",
        i128 => "serialize_i128",
        u8 => "serialize_u8",
        u16 => "serialize_u16",
        u32 => "serialize_u32",
        u64 => "serialize_u64",
        u128 => "serialize_u128",
        f32 => "serialize_f32",
        f64 => "serialize_f64",
        char => "serialize_char",
        ::std::string::String => "serialize_str",
    }

    impl<T> Serialize for ::std::vec::Vec<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let _ = (self, serializer);
            Err(S::Error::unsupported("serialize_vec"))
        }
    }

    impl<A, B> Serialize for (A, B)
    where
        A: Serialize,
        B: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let _ = (self, serializer);
            Err(S::Error::unsupported("serialize_tuple2"))
        }
    }

    impl<K, V> Serialize for ::std::collections::HashMap<K, V>
    where
        K: Serialize,
        V: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let _ = (self, serializer);
            Err(S::Error::unsupported("serialize_hashmap"))
        }
    }

    impl<T, const N: usize> Serialize for [T; N]
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let _ = (self, serializer);
            Err(S::Error::unsupported("serialize_array"))
        }
    }

    impl<T> Serialize for &T
    where
        T: Serialize + ?Sized,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            (*self).serialize(serializer)
        }
    }

    impl<T> Serialize for &mut T
    where
        T: Serialize + ?Sized,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            (**self).serialize(serializer)
        }
    }
}

pub mod de {
    use super::StubError;
    use core::fmt;

    pub trait Deserialize<'de>: Sized {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>;
    }

    pub trait DeserializeOwned: for<'de> Deserialize<'de> {}

    impl<T> DeserializeOwned for T where T: for<'de> Deserialize<'de> {}

    pub trait Deserializer<'de>: Sized {
        type Error: Error;

        fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(Self::Error::unsupported("deserialize_any"))
        }

        fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_unit_struct<V>(
            self,
            _name: &'static str,
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_newtype_struct<V>(
            self,
            _name: &'static str,
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_tuple_struct<V>(
            self,
            _name: &'static str,
            _len: usize,
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_struct<V>(
            self,
            _name: &'static str,
            _fields: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_enum<V>(
            self,
            _name: &'static str,
            _variants: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            self.deserialize_any(visitor)
        }

        fn is_human_readable(&self) -> bool {
            true
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Unexpected<'a> {
        Bool(bool),
        Unsigned(u64),
        Signed(i64),
        Float(f64),
        Char(char),
        Str(&'a str),
        Bytes(&'a [u8]),
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

    impl<'a> fmt::Display for Unexpected<'a> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            use Unexpected::*;
            match *self {
                Bool(value) => write!(formatter, "boolean `{value}`"),
                Unsigned(value) => write!(formatter, "integer `{value}`"),
                Signed(value) => write!(formatter, "integer `{value}`"),
                Float(value) => write!(formatter, "floating point `{value}`"),
                Char(value) => write!(formatter, "character `{value}`"),
                Str(value) => write!(formatter, "string {value:?}"),
                Bytes(_) => write!(formatter, "byte array"),
                Unit => formatter.write_str("unit value"),
                Option => formatter.write_str("Option value"),
                NewtypeStruct => formatter.write_str("newtype struct"),
                Seq => formatter.write_str("sequence"),
                Map => formatter.write_str("map"),
                Enum => formatter.write_str("enum"),
                UnitVariant => formatter.write_str("unit variant"),
                NewtypeVariant => formatter.write_str("newtype variant"),
                TupleVariant => formatter.write_str("tuple variant"),
                StructVariant => formatter.write_str("struct variant"),
                Other(message) => formatter.write_str(message),
            }
        }
    }

    pub trait Expected {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;
    }

    struct DisplayExpected<'a>(&'a dyn Expected);

    impl fmt::Display for DisplayExpected<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fmt(formatter)
        }
    }

    struct OneOf {
        names: &'static [&'static str],
    }

    impl fmt::Display for OneOf {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.names {
                [] => formatter.write_str(""),
                [single] => write!(formatter, "`{single}`"),
                [first, rest @ ..] => {
                    write!(formatter, "`{first}`")?;
                    for (index, name) in rest.iter().enumerate() {
                        if index + 1 == rest.len() {
                            write!(formatter, " or `{name}`")?;
                        } else {
                            write!(formatter, ", `{name}`")?;
                        }
                    }
                    Ok(())
                }
            }
        }
    }

    pub trait Visitor<'de>: Sized {
        type Value;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;

        fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_bool"))
        }

        fn visit_i8<E>(self, _value: i8) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_i8"))
        }

        fn visit_i16<E>(self, _value: i16) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_i16"))
        }

        fn visit_i32<E>(self, _value: i32) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_i32"))
        }

        fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_i64"))
        }

        fn visit_i128<E>(self, _value: i128) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_i128"))
        }

        fn visit_u8<E>(self, _value: u8) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_u8"))
        }

        fn visit_u16<E>(self, _value: u16) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_u16"))
        }

        fn visit_u32<E>(self, _value: u32) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_u32"))
        }

        fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_u64"))
        }

        fn visit_u128<E>(self, _value: u128) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_u128"))
        }

        fn visit_f32<E>(self, _value: f32) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_f32"))
        }

        fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_f64"))
        }

        fn visit_char<E>(self, _value: char) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_char"))
        }

        fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_str"))
        }

        fn visit_borrowed_str<E>(self, _value: &'de str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_borrowed_str"))
        }

        fn visit_string<E>(self, _value: String) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_string"))
        }

        fn visit_bytes<E>(self, _value: &[u8]) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_bytes"))
        }

        fn visit_borrowed_bytes<E>(self, _value: &'de [u8]) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_borrowed_bytes"))
        }

        fn visit_byte_buf<E>(self, _value: Vec<u8>) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_byte_buf"))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_none"))
        }

        fn visit_some<D>(self, _deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            Err(D::Error::unsupported("visit_some"))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Err(E::unsupported("visit_unit"))
        }

        fn visit_newtype_struct<D>(self, _deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            Err(D::Error::unsupported("visit_newtype_struct"))
        }

        fn visit_seq<A>(self, _seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            Err(A::Error::unsupported("visit_seq"))
        }

        fn visit_map<A>(self, _map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            Err(A::Error::unsupported("visit_map"))
        }

        fn visit_enum<A>(self, _data: A) -> Result<Self::Value, A::Error>
        where
            A: EnumAccess<'de>,
        {
            Err(A::Error::unsupported("visit_enum"))
        }
    }

    impl<'de, T> Expected for T
    where
        T: Visitor<'de>,
    {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.expecting(formatter)
        }
    }

    impl<'a> Expected for &'a str {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self)
        }
    }

    pub trait SeqAccess<'de> {
        type Error: Error;

        fn next_element_seed<T>(&mut self, _seed: T) -> Result<Option<T::Value>, Self::Error>
        where
            T: DeserializeSeed<'de>,
        {
            Err(Self::Error::unsupported("next_element_seed"))
        }

        fn next_element<T>(&mut self) -> Result<Option<T>, Self::Error>
        where
            T: Deserialize<'de>,
        {
            self.next_element_seed(<T as IntoSeed<'de>>::from_type())
        }

        fn size_hint(&self) -> Option<usize> {
            None
        }
    }

    pub trait MapAccess<'de> {
        type Error: Error;

        fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, Self::Error>
        where
            K: DeserializeSeed<'de>,
        {
            Err(Self::Error::unsupported("next_key_seed"))
        }

        fn next_value_seed<V>(&mut self, _seed: V) -> Result<V::Value, Self::Error>
        where
            V: DeserializeSeed<'de>,
        {
            Err(Self::Error::unsupported("next_value_seed"))
        }

        fn next_key<K>(&mut self) -> Result<Option<K>, Self::Error>
        where
            K: Deserialize<'de>,
        {
            match self.next_key_seed(<K as IntoSeed<'de>>::from_type())? {
                Some(value) => Ok(Some(value)),
                None => Ok(None),
            }
        }

        fn next_value<V>(&mut self) -> Result<V, Self::Error>
        where
            V: Deserialize<'de>,
        {
            self.next_value_seed(<V as IntoSeed<'de>>::from_type())
        }

        fn next_entry<K, V>(&mut self) -> Result<Option<(K, V)>, Self::Error>
        where
            K: Deserialize<'de>,
            V: Deserialize<'de>,
        {
            match self.next_key::<K>()? {
                Some(key) => {
                    let value = self.next_value::<V>()?;
                    Ok(Some((key, value)))
                }
                None => Ok(None),
            }
        }

        fn size_hint(&self) -> Option<usize> {
            None
        }
    }

    pub trait EnumAccess<'de>: Sized {
        type Error: Error;
        type Variant: VariantAccess<'de, Error = Self::Error>;

        fn variant_seed<V>(self, _seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
        where
            V: DeserializeSeed<'de>,
        {
            Err(Self::Error::unsupported("variant_seed"))
        }

        fn variant<V>(self) -> Result<(V, Self::Variant), Self::Error>
        where
            V: Deserialize<'de>,
        {
            let seed = <V as IntoSeed<'de>>::from_type();
            let (value, variant) = self.variant_seed(seed)?;
            Ok((value, variant))
        }
    }

    pub trait VariantAccess<'de>: Sized {
        type Error: Error;

        fn unit_variant(self) -> Result<(), Self::Error> {
            Err(Self::Error::unsupported("unit_variant"))
        }

        fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Self::Error>
        where
            T: DeserializeSeed<'de>,
        {
            Err(Self::Error::unsupported("newtype_variant_seed"))
        }

        fn newtype_variant<T>(self) -> Result<T, Self::Error>
        where
            T: Deserialize<'de>,
        {
            self.newtype_variant_seed(<T as IntoSeed<'de>>::from_type())
        }

        fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(Self::Error::unsupported("tuple_variant"))
        }

        fn struct_variant<V>(
            self,
            _fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(Self::Error::unsupported("struct_variant"))
        }
    }

    pub trait DeserializeSeed<'de>: Sized {
        type Value;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>;
    }

    pub trait IntoDeserializer<'de, E>: Sized {
        type Deserializer: Deserializer<'de, Error = E>;

        fn into_deserializer(self) -> Self::Deserializer;
    }

    pub trait Error: std::error::Error + Sized {
        fn custom<T: fmt::Display>(msg: T) -> Self;

        fn invalid_type(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
            Self::custom(format!(
                "invalid type: {}, expected {}",
                unexpected,
                DisplayExpected(expected)
            ))
        }

        fn invalid_value(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
            Self::custom(format!(
                "invalid value: {}, expected {}",
                unexpected,
                DisplayExpected(expected)
            ))
        }

        fn invalid_length(len: usize, expected: &dyn Expected) -> Self {
            Self::custom(format!(
                "invalid length {len}, expected {}",
                DisplayExpected(expected)
            ))
        }

        fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
            if expected.is_empty() {
                Self::custom(format!(
                    "unknown variant `{variant}`, there are no variants"
                ))
            } else {
                Self::custom(format!(
                    "unknown variant `{variant}`, expected {}",
                    OneOf { names: expected }
                ))
            }
        }

        fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
            if expected.is_empty() {
                Self::custom(format!("unknown field `{field}`, there are no fields"))
            } else {
                Self::custom(format!(
                    "unknown field `{field}`, expected {}",
                    OneOf { names: expected }
                ))
            }
        }

        fn missing_field(field: &'static str) -> Self {
            Self::custom(format!("missing field `{field}`"))
        }

        fn duplicate_field(field: &'static str) -> Self {
            Self::custom(format!("duplicate field `{field}`"))
        }

        fn unsupported(op: &'static str) -> Self {
            Self::custom(format_args!("foundation_serde stub cannot {op}").to_string())
        }
    }

    impl Error for StubError {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            StubError::new(msg)
        }
    }

    macro_rules! unsupported_deserialize {
        ($($ty:ty => $op:literal),* $(,)?) => {
            $(impl<'de> Deserialize<'de> for $ty {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    let _ = deserializer;
                    Err(D::Error::unsupported($op))
                }
            })*
        };
    }

    unsupported_deserialize! {
        bool => "deserialize_bool",
        i8 => "deserialize_i8",
        i16 => "deserialize_i16",
        i32 => "deserialize_i32",
        i64 => "deserialize_i64",
        i128 => "deserialize_i128",
        u8 => "deserialize_u8",
        u16 => "deserialize_u16",
        u32 => "deserialize_u32",
        u64 => "deserialize_u64",
        u128 => "deserialize_u128",
        f32 => "deserialize_f32",
        f64 => "deserialize_f64",
        char => "deserialize_char",
        ::std::string::String => "deserialize_string",
    }

    impl<'de, T> Deserialize<'de> for ::std::vec::Vec<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let _ = deserializer;
            Err(D::Error::unsupported("deserialize_vec"))
        }
    }

    impl<'de, A, B> Deserialize<'de> for (A, B)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let _ = deserializer;
            Err(D::Error::unsupported("deserialize_tuple2"))
        }
    }

    impl<'de, K, V> Deserialize<'de> for ::std::collections::HashMap<K, V>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash,
        V: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let _ = deserializer;
            Err(D::Error::unsupported("deserialize_hashmap"))
        }
    }

    impl<'de, T, const N: usize> Deserialize<'de> for [T; N]
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let _ = deserializer;
            Err(D::Error::unsupported("deserialize_array"))
        }
    }

    use core::marker::PhantomData;

    pub struct UnsupportedDeserializer<E> {
        op: &'static str,
        marker: PhantomData<E>,
    }

    impl<E> UnsupportedDeserializer<E> {
        pub fn new(op: &'static str) -> Self {
            Self {
                op,
                marker: PhantomData,
            }
        }
    }

    impl<'de, E> Deserializer<'de> for UnsupportedDeserializer<E>
    where
        E: Error,
    {
        type Error = E;

        fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_bool<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_i8<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_i16<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_i32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_i64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_i128<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_u8<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_u16<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_u32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_u64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_u128<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_f32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_f64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_str<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_string<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_option<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_unit_struct<V>(
            self,
            _name: &'static str,
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_newtype_struct<V>(
            self,
            _name: &'static str,
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_tuple_struct<V>(
            self,
            _name: &'static str,
            _len: usize,
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_struct<V>(
            self,
            _name: &'static str,
            _fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_enum<V>(
            self,
            _name: &'static str,
            _variants: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            Err(E::unsupported(self.op))
        }

        fn is_human_readable(&self) -> bool {
            true
        }
    }

    macro_rules! unsupported_into_deserializer {
        ($($ty:ty),* $(,)?) => {
            $(impl<'de, E> IntoDeserializer<'de, E> for $ty
            where
                E: Error,
            {
                type Deserializer = UnsupportedDeserializer<E>;

                fn into_deserializer(self) -> Self::Deserializer {
                    let _ = self;
                    UnsupportedDeserializer::new("into_deserializer")
                }
            })*
        };
    }

    unsupported_into_deserializer! {
        bool, i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, f32, f64, char
    }

    pub mod value {
        use super::{Deserializer, Error, IntoDeserializer, Visitor};
        use core::marker::PhantomData;
        use std::string::String;

        #[derive(Clone)]
        pub struct StringDeserializer<E> {
            value: String,
            marker: PhantomData<E>,
        }

        impl<E> StringDeserializer<E> {
            pub fn new(value: impl Into<String>) -> Self {
                Self {
                    value: value.into(),
                    marker: PhantomData,
                }
            }
        }

        impl<'de, E> IntoDeserializer<'de, E> for String
        where
            E: Error,
        {
            type Deserializer = StringDeserializer<E>;

            fn into_deserializer(self) -> Self::Deserializer {
                StringDeserializer::new(self)
            }
        }

        impl<'de, E> IntoDeserializer<'de, E> for StringDeserializer<E>
        where
            E: Error,
        {
            type Deserializer = Self;

            fn into_deserializer(self) -> Self::Deserializer {
                self
            }
        }

        impl<'de, E> Deserializer<'de> for StringDeserializer<E>
        where
            E: Error,
        {
            type Error = E;

            fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                visitor.visit_string(self.value)
            }

            fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                visitor.visit_string(self.value)
            }

            fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                visitor.visit_string(self.value)
            }

            fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                visitor.visit_string(self.value)
            }
        }
    }

    trait IntoSeed<'de>: Deserialize<'de> {
        type Seed: DeserializeSeed<'de, Value = Self>;

        fn from_type() -> Self::Seed;
    }

    impl<'de, T> IntoSeed<'de> for T
    where
        T: Deserialize<'de>,
    {
        type Seed = PhantomSeed<T>;

        fn from_type() -> Self::Seed {
            PhantomSeed(core::marker::PhantomData)
        }
    }

    struct PhantomSeed<T>(core::marker::PhantomData<T>);

    impl<'de, T> DeserializeSeed<'de> for PhantomSeed<T>
    where
        T: Deserialize<'de>,
    {
        type Value = T;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            T::deserialize(deserializer)
        }
    }
}

#[derive(Debug, Clone)]
pub struct StubError {
    message: String,
}

impl StubError {
    pub fn new(msg: impl fmt::Display) -> Self {
        Self {
            message: msg.to_string(),
        }
    }

    fn unsupported(op: &'static str) -> Self {
        Self::new(format!("foundation_serde stub cannot {op}"))
    }
}

impl fmt::Display for StubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StubError {}

pub use de::{Deserialize, DeserializeOwned, Deserializer, Error, Expected, Unexpected};
pub use ser::{Serialize, Serializer};

pub mod serde {
    pub use super::de;
    pub use super::ser;
    pub use super::{Deserialize, DeserializeOwned, Error, Expected, Serialize, Unexpected};
}
