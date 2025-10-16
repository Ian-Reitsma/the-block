#![allow(dead_code)]

use core::fmt;

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

    macro_rules! serialize_primitive {
        ($($ty:ty => $method:ident),+ $(,)?) => {
            $(impl Serialize for $ty {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    serializer.$method(*self)
                }
            })+
        };
    }

    serialize_primitive! {
        bool => serialize_bool,
        i8 => serialize_i8,
        i16 => serialize_i16,
        i32 => serialize_i32,
        i64 => serialize_i64,
        i128 => serialize_i128,
        u8 => serialize_u8,
        u16 => serialize_u16,
        u32 => serialize_u32,
        u64 => serialize_u64,
        u128 => serialize_u128,
        f32 => serialize_f32,
        f64 => serialize_f64,
    }

    impl Serialize for isize {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_i64(*self as i64)
        }
    }

    impl Serialize for usize {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_u64(*self as u64)
        }
    }

    impl Serialize for char {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_char(*self)
        }
    }

    impl Serialize for ::std::string::String {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(self)
        }
    }

    impl Serialize for str {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(self)
        }
    }

    impl Serialize for () {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_unit()
        }
    }

    impl<T> Serialize for Option<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                Some(value) => serializer.serialize_some(value),
                None => serializer.serialize_none(),
            }
        }
    }

    impl<T, E> Serialize for Result<T, E>
    where
        T: Serialize,
        E: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                Ok(value) => serializer.serialize_newtype_variant("Result", 0, "Ok", value),
                Err(err) => serializer.serialize_newtype_variant("Result", 1, "Err", err),
            }
        }
    }

    impl<T> Serialize for ::std::vec::Vec<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut seq = serializer.serialize_seq(Some(self.len()))?;
            for value in self {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
        }
    }

    impl<T> Serialize for ::std::collections::VecDeque<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut seq = serializer.serialize_seq(Some(self.len()))?;
            for value in self {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
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
            let mut tuple = serializer.serialize_tuple(2)?;
            SerializeTuple::serialize_element(&mut tuple, &self.0)?;
            SerializeTuple::serialize_element(&mut tuple, &self.1)?;
            tuple.end()
        }
    }

    impl<A, B, C> Serialize for (A, B, C)
    where
        A: Serialize,
        B: Serialize,
        C: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut tuple = serializer.serialize_tuple(3)?;
            SerializeTuple::serialize_element(&mut tuple, &self.0)?;
            SerializeTuple::serialize_element(&mut tuple, &self.1)?;
            SerializeTuple::serialize_element(&mut tuple, &self.2)?;
            tuple.end()
        }
    }

    impl<A, B, C, D> Serialize for (A, B, C, D)
    where
        A: Serialize,
        B: Serialize,
        C: Serialize,
        D: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut tuple = serializer.serialize_tuple(4)?;
            SerializeTuple::serialize_element(&mut tuple, &self.0)?;
            SerializeTuple::serialize_element(&mut tuple, &self.1)?;
            SerializeTuple::serialize_element(&mut tuple, &self.2)?;
            SerializeTuple::serialize_element(&mut tuple, &self.3)?;
            tuple.end()
        }
    }

    impl<A, B, C, D, E> Serialize for (A, B, C, D, E)
    where
        A: Serialize,
        B: Serialize,
        C: Serialize,
        D: Serialize,
        E: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut tuple = serializer.serialize_tuple(5)?;
            SerializeTuple::serialize_element(&mut tuple, &self.0)?;
            SerializeTuple::serialize_element(&mut tuple, &self.1)?;
            SerializeTuple::serialize_element(&mut tuple, &self.2)?;
            SerializeTuple::serialize_element(&mut tuple, &self.3)?;
            SerializeTuple::serialize_element(&mut tuple, &self.4)?;
            tuple.end()
        }
    }

    impl<A, B, C, D, E, F> Serialize for (A, B, C, D, E, F)
    where
        A: Serialize,
        B: Serialize,
        C: Serialize,
        D: Serialize,
        E: Serialize,
        F: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut tuple = serializer.serialize_tuple(6)?;
            SerializeTuple::serialize_element(&mut tuple, &self.0)?;
            SerializeTuple::serialize_element(&mut tuple, &self.1)?;
            SerializeTuple::serialize_element(&mut tuple, &self.2)?;
            SerializeTuple::serialize_element(&mut tuple, &self.3)?;
            SerializeTuple::serialize_element(&mut tuple, &self.4)?;
            SerializeTuple::serialize_element(&mut tuple, &self.5)?;
            tuple.end()
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
            let mut map = serializer.serialize_map(Some(self.len()))?;
            for (key, value) in self {
                SerializeMap::serialize_key(&mut map, key)?;
                SerializeMap::serialize_value(&mut map, value)?;
            }
            map.end()
        }
    }

    impl<K, V> Serialize for ::std::collections::BTreeMap<K, V>
    where
        K: Serialize,
        V: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut map = serializer.serialize_map(Some(self.len()))?;
            for (key, value) in self {
                SerializeMap::serialize_key(&mut map, key)?;
                SerializeMap::serialize_value(&mut map, value)?;
            }
            map.end()
        }
    }

    impl<T> Serialize for ::std::collections::HashSet<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut seq = serializer.serialize_seq(Some(self.len()))?;
            for value in self {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
        }
    }

    impl<T> Serialize for ::std::collections::BTreeSet<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut seq = serializer.serialize_seq(Some(self.len()))?;
            for value in self {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
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
            let mut seq = serializer.serialize_seq(Some(N))?;
            for value in self {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
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

    impl<'a, T> Serialize for ::std::slice::Iter<'a, T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut iter = self.clone();
            let mut seq = serializer.serialize_seq(None)?;
            while let Some(value) = iter.next() {
                SerializeSeq::serialize_element(&mut seq, value)?;
            }
            seq.end()
        }
    }
}

pub mod de {
    use super::StubError;
    use core::fmt;
    use core::marker::PhantomData;

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

    struct BoolVisitor;

    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a boolean")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }
    }

    impl<'de> Deserialize<'de> for bool {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bool(BoolVisitor)
        }
    }

    macro_rules! deserialize_signed {
        ($($ty:ty => $name:ident),+ $(,)?) => {
            $(
                struct $name;

                impl<'de> Visitor<'de> for $name {
                    type Value = $ty;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str(stringify!($ty))
                    }

                    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
                    where
                        E: Error,
                    {
                        if value >= <$ty>::MIN as i64 && value <= <$ty>::MAX as i64 {
                            Ok(value as $ty)
                        } else {
                            Err(E::invalid_value(Unexpected::Signed(value), &self))
                        }
                    }

                    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
                    where
                        E: Error,
                    {
                        if value <= <$ty>::MAX as u64 {
                            Ok(value as $ty)
                        } else {
                            Err(E::invalid_value(Unexpected::Unsigned(value), &self))
                        }
                    }
                }

                impl<'de> Deserialize<'de> for $ty {
                    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        deserializer.deserialize_i64($name)
                    }
                }
            )+
        };
    }

    deserialize_signed! {
        i8 => I8Visitor,
        i16 => I16Visitor,
        i32 => I32Visitor,
        i64 => I64Visitor,
        isize => IsizeVisitor
    }

    struct I128Visitor;

    impl<'de> Visitor<'de> for I128Visitor {
        type Value = i128;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a 128-bit signed integer")
        }

        fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as i128)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as i128)
        }
    }

    impl<'de> Deserialize<'de> for i128 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_i64(I128Visitor)
        }
    }

    macro_rules! deserialize_unsigned {
        ($($ty:ty => $name:ident),+ $(,)?) => {
            $(
                struct $name;

                impl<'de> Visitor<'de> for $name {
                    type Value = $ty;

                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str(stringify!($ty))
                    }

                    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
                    where
                        E: Error,
                    {
                        if value <= <$ty>::MAX as u64 {
                            Ok(value as $ty)
                        } else {
                            Err(E::invalid_value(Unexpected::Unsigned(value), &self))
                        }
                    }

                    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
                    where
                        E: Error,
                    {
                        if value >= 0 && (value as u64) <= <$ty>::MAX as u64 {
                            Ok(value as $ty)
                        } else {
                            Err(E::invalid_value(Unexpected::Signed(value), &self))
                        }
                    }
                }

                impl<'de> Deserialize<'de> for $ty {
                    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        deserializer.deserialize_u64($name)
                    }
                }
            )+
        };
    }

    deserialize_unsigned! {
        u8 => U8Visitor,
        u16 => U16Visitor,
        u32 => U32Visitor,
        u64 => U64Visitor,
        usize => UsizeVisitor
    }

    struct U128Visitor;

    impl<'de> Visitor<'de> for U128Visitor {
        type Value = u128;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a 128-bit unsigned integer")
        }

        fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as u128)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if value >= 0 {
                Ok(value as u128)
            } else {
                Err(E::invalid_value(Unexpected::Signed(value), &self))
            }
        }
    }

    impl<'de> Deserialize<'de> for u128 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_u128(U128Visitor)
        }
    }

    struct F32Visitor;

    impl<'de> Visitor<'de> for F32Visitor {
        type Value = f32;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a 32-bit float")
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as f32)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as f32)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as f32)
        }
    }

    impl<'de> Deserialize<'de> for f32 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_f64(F32Visitor)
        }
    }

    struct F64Visitor;

    impl<'de> Visitor<'de> for F64Visitor {
        type Value = f64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a 64-bit float")
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as f64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value as f64)
        }
    }

    impl<'de> Deserialize<'de> for f64 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_f64(F64Visitor)
        }
    }

    struct CharVisitor;

    impl<'de> Visitor<'de> for CharVisitor {
        type Value = char;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a character")
        }

        fn visit_char<E>(self, value: char) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            let mut chars = value.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                Ok(ch)
            } else {
                Err(E::invalid_value(Unexpected::Str(value), &self))
            }
        }
    }

    impl<'de> Deserialize<'de> for char {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_char(CharVisitor)
        }
    }

    struct StringVisitor;

    impl<'de> Visitor<'de> for StringVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string")
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value.to_owned())
        }
    }

    impl<'de> Deserialize<'de> for String {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_string(StringVisitor)
        }
    }

    struct UnitVisitor;

    impl<'de> Visitor<'de> for UnitVisitor {
        type Value = ();

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("unit value")
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(())
        }
    }

    impl<'de> Deserialize<'de> for () {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_unit(UnitVisitor)
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::option::Option<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct OptionVisitor<T>(PhantomData<T>);

            impl<'de, T> Visitor<'de> for OptionVisitor<T>
            where
                T: Deserialize<'de>,
            {
                type Value = Option<T>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("an optional value")
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
            }

            deserializer.deserialize_option(OptionVisitor(PhantomData))
        }
    }

    impl<'de, T, E> Deserialize<'de> for Result<T, E>
    where
        T: Deserialize<'de>,
        E: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            enum Variant<T, E> {
                Ok(T),
                Err(E),
            }

            struct ResultVisitor<T, E>(PhantomData<(T, E)>);

            impl<'de, T, E> Visitor<'de> for ResultVisitor<T, E>
            where
                T: Deserialize<'de>,
                E: Deserialize<'de>,
            {
                type Value = Result<T, E>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a result enum")
                }

                fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
                where
                    A: EnumAccess<'de>,
                {
                    let (variant, value) = data.variant::<String>()?;
                    match variant.as_str() {
                        "Ok" => value.newtype_variant().map(Result::Ok),
                        "Err" => value.newtype_variant().map(Result::Err),
                        _ => Err(A::Error::unknown_variant(variant.as_ref(), &["Ok", "Err"])),
                    }
                }
            }

            deserializer.deserialize_enum("Result", &["Ok", "Err"], ResultVisitor(PhantomData))
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::vec::Vec<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct VecVisitor<T>(PhantomData<T>);

            impl<'de, T> Visitor<'de> for VecVisitor<T>
            where
                T: Deserialize<'de>,
            {
                type Value = Vec<T>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

            deserializer.deserialize_seq(VecVisitor(PhantomData))
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::collections::VecDeque<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct VecDequeVisitor<T>(PhantomData<T>);

            impl<'de, T> Visitor<'de> for VecDequeVisitor<T>
            where
                T: Deserialize<'de>,
            {
                type Value = std::collections::VecDeque<T>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a sequence")
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut values =
                        std::collections::VecDeque::with_capacity(seq.size_hint().unwrap_or(0));
                    while let Some(value) = seq.next_element()? {
                        values.push_back(value);
                    }
                    Ok(values)
                }
            }

            deserializer.deserialize_seq(VecDequeVisitor(PhantomData))
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::boxed::Box<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            T::deserialize(deserializer).map(Box::new)
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
            struct ArrayVisitor<T, const N: usize>(PhantomData<T>);

            impl<'de, T, const N: usize> Visitor<'de> for ArrayVisitor<T, N>
            where
                T: Deserialize<'de>,
            {
                type Value = [T; N];

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(formatter, "an array of length {N}")
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut values = Vec::with_capacity(N);
                    while let Some(value) = seq.next_element()? {
                        values.push(value);
                    }
                    if values.len() != N {
                        return Err(A::Error::invalid_length(values.len(), &self));
                    }
                    let mut iter = values.into_iter();
                    Ok(core::array::from_fn(|_| {
                        iter.next().expect("length checked")
                    }))
                }
            }

            deserializer.deserialize_tuple(N, ArrayVisitor::<T, N>(PhantomData))
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
            struct MapVisitor<K, V>(PhantomData<(K, V)>);

            impl<'de, K, V> Visitor<'de> for MapVisitor<K, V>
            where
                K: Deserialize<'de> + Eq + std::hash::Hash,
                V: Deserialize<'de>,
            {
                type Value = std::collections::HashMap<K, V>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

            deserializer.deserialize_map(MapVisitor(PhantomData))
        }
    }

    impl<'de, K, V> Deserialize<'de> for ::std::collections::BTreeMap<K, V>
    where
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct BTreeMapVisitor<K, V>(PhantomData<(K, V)>);

            impl<'de, K, V> Visitor<'de> for BTreeMapVisitor<K, V>
            where
                K: Deserialize<'de> + Ord,
                V: Deserialize<'de>,
            {
                type Value = std::collections::BTreeMap<K, V>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

            deserializer.deserialize_map(BTreeMapVisitor(PhantomData))
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::collections::HashSet<T>
    where
        T: Deserialize<'de> + Eq + std::hash::Hash,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct HashSetVisitor<T>(PhantomData<T>);

            impl<'de, T> Visitor<'de> for HashSetVisitor<T>
            where
                T: Deserialize<'de> + Eq + std::hash::Hash,
            {
                type Value = std::collections::HashSet<T>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

            deserializer.deserialize_seq(HashSetVisitor(PhantomData))
        }
    }

    impl<'de, T> Deserialize<'de> for ::std::collections::BTreeSet<T>
    where
        T: Deserialize<'de> + Ord,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct BTreeSetVisitor<T>(PhantomData<T>);

            impl<'de, T> Visitor<'de> for BTreeSetVisitor<T>
            where
                T: Deserialize<'de> + Ord,
            {
                type Value = std::collections::BTreeSet<T>;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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

            deserializer.deserialize_seq(BTreeSetVisitor(PhantomData))
        }
    }

    impl<'de, A, B> Deserialize<'de> for (A, B)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: Deserializer<'de>,
        {
            struct Tuple2Visitor<A, B>(PhantomData<(A, B)>);

            impl<'de, A, B> Visitor<'de> for Tuple2Visitor<A, B>
            where
                A: Deserialize<'de>,
                B: Deserialize<'de>,
            {
                type Value = (A, B);

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a two-element tuple")
                }

                fn visit_seq<Access>(self, mut seq: Access) -> Result<Self::Value, Access::Error>
                where
                    Access: SeqAccess<'de>,
                {
                    let first = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(0, &self))?;
                    let second = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(1, &self))?;
                    Ok((first, second))
                }
            }

            deserializer.deserialize_tuple(2, Tuple2Visitor(PhantomData))
        }
    }

    impl<'de, A, B, C> Deserialize<'de> for (A, B, C)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
        C: Deserialize<'de>,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: Deserializer<'de>,
        {
            struct Tuple3Visitor<A, B, C>(PhantomData<(A, B, C)>);

            impl<'de, A, B, C> Visitor<'de> for Tuple3Visitor<A, B, C>
            where
                A: Deserialize<'de>,
                B: Deserialize<'de>,
                C: Deserialize<'de>,
            {
                type Value = (A, B, C);

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a three-element tuple")
                }

                fn visit_seq<Access>(self, mut seq: Access) -> Result<Self::Value, Access::Error>
                where
                    Access: SeqAccess<'de>,
                {
                    let first = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(0, &self))?;
                    let second = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(1, &self))?;
                    let third = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(2, &self))?;
                    Ok((first, second, third))
                }
            }

            deserializer.deserialize_tuple(3, Tuple3Visitor(PhantomData))
        }
    }

    impl<'de, A, B, C, D> Deserialize<'de> for (A, B, C, D)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
        C: Deserialize<'de>,
        D: Deserialize<'de>,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: Deserializer<'de>,
        {
            struct Tuple4Visitor<A, B, C, D>(PhantomData<(A, B, C, D)>);

            impl<'de, A, B, C, D> Visitor<'de> for Tuple4Visitor<A, B, C, D>
            where
                A: Deserialize<'de>,
                B: Deserialize<'de>,
                C: Deserialize<'de>,
                D: Deserialize<'de>,
            {
                type Value = (A, B, C, D);

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a four-element tuple")
                }

                fn visit_seq<Access>(self, mut seq: Access) -> Result<Self::Value, Access::Error>
                where
                    Access: SeqAccess<'de>,
                {
                    let first = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(0, &self))?;
                    let second = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(1, &self))?;
                    let third = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(2, &self))?;
                    let fourth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(3, &self))?;
                    Ok((first, second, third, fourth))
                }
            }

            deserializer.deserialize_tuple(4, Tuple4Visitor(PhantomData))
        }
    }

    impl<'de, A, B, C, D, E> Deserialize<'de> for (A, B, C, D, E)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
        C: Deserialize<'de>,
        D: Deserialize<'de>,
        E: Deserialize<'de>,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: Deserializer<'de>,
        {
            struct Tuple5Visitor<A, B, C, D, E>(PhantomData<(A, B, C, D, E)>);

            impl<'de, A, B, C, D, E> Visitor<'de> for Tuple5Visitor<A, B, C, D, E>
            where
                A: Deserialize<'de>,
                B: Deserialize<'de>,
                C: Deserialize<'de>,
                D: Deserialize<'de>,
                E: Deserialize<'de>,
            {
                type Value = (A, B, C, D, E);

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a five-element tuple")
                }

                fn visit_seq<Access>(self, mut seq: Access) -> Result<Self::Value, Access::Error>
                where
                    Access: SeqAccess<'de>,
                {
                    let first = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(0, &self))?;
                    let second = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(1, &self))?;
                    let third = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(2, &self))?;
                    let fourth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(3, &self))?;
                    let fifth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(4, &self))?;
                    Ok((first, second, third, fourth, fifth))
                }
            }

            deserializer.deserialize_tuple(5, Tuple5Visitor(PhantomData))
        }
    }

    impl<'de, A, B, C, D, E, F> Deserialize<'de> for (A, B, C, D, E, F)
    where
        A: Deserialize<'de>,
        B: Deserialize<'de>,
        C: Deserialize<'de>,
        D: Deserialize<'de>,
        E: Deserialize<'de>,
        F: Deserialize<'de>,
    {
        fn deserialize<Des>(deserializer: Des) -> Result<Self, Des::Error>
        where
            Des: Deserializer<'de>,
        {
            struct Tuple6Visitor<A, B, C, D, E, F>(PhantomData<(A, B, C, D, E, F)>);

            impl<'de, A, B, C, D, E, F> Visitor<'de> for Tuple6Visitor<A, B, C, D, E, F>
            where
                A: Deserialize<'de>,
                B: Deserialize<'de>,
                C: Deserialize<'de>,
                D: Deserialize<'de>,
                E: Deserialize<'de>,
                F: Deserialize<'de>,
            {
                type Value = (A, B, C, D, E, F);

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("a six-element tuple")
                }

                fn visit_seq<Access>(self, mut seq: Access) -> Result<Self::Value, Access::Error>
                where
                    Access: SeqAccess<'de>,
                {
                    let first = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(0, &self))?;
                    let second = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(1, &self))?;
                    let third = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(2, &self))?;
                    let fourth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(3, &self))?;
                    let fifth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(4, &self))?;
                    let sixth = seq
                        .next_element()?
                        .ok_or_else(|| Access::Error::invalid_length(5, &self))?;
                    Ok((first, second, third, fourth, fifth, sixth))
                }
            }

            deserializer.deserialize_tuple(6, Tuple6Visitor(PhantomData))
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
