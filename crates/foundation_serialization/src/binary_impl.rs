use core::fmt;
use std::string::FromUtf8Error;

use crate::Serialize;
use serde::de::value::{IntoDeserializer, StringDeserializer};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};

// Note: "serde" is aliased to foundation_serde in Cargo.toml - all imports above
// now refer to our first-party traits!

pub type Result<T> = std::result::Result<T, Error>;

/// Error raised by the in-house binary encoder/decoder.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    Message(String),
    UnexpectedEof,
    InvalidBool(u8),
    InvalidUtf8(FromUtf8Error),
    LengthOverflow,
    NonFiniteFloat,
}

impl Error {
    fn message<T: fmt::Display>(msg: T) -> Self {
        Self {
            kind: ErrorKind::Message(msg.to_string()),
        }
    }

    fn unexpected_eof() -> Self {
        Self {
            kind: ErrorKind::UnexpectedEof,
        }
    }

    fn invalid_bool(value: u8) -> Self {
        Self {
            kind: ErrorKind::InvalidBool(value),
        }
    }

    fn invalid_utf8(err: FromUtf8Error) -> Self {
        Self {
            kind: ErrorKind::InvalidUtf8(err),
        }
    }

    fn length_overflow() -> Self {
        Self {
            kind: ErrorKind::LengthOverflow,
        }
    }

    fn non_finite_float() -> Self {
        Self {
            kind: ErrorKind::NonFiniteFloat,
        }
    }
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::message(msg)
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::message(msg)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Message(msg) => write!(f, "{msg}"),
            ErrorKind::UnexpectedEof => write!(f, "unexpected end of input"),
            ErrorKind::InvalidBool(value) => write!(f, "invalid boolean discriminant: {value}"),
            ErrorKind::InvalidUtf8(err) => write!(f, "invalid utf-8 string: {err}"),
            ErrorKind::LengthOverflow => write!(f, "length exceeds u64::MAX"),
            ErrorKind::NonFiniteFloat => {
                write!(f, "non-finite floating point numbers are unsupported")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::InvalidUtf8(err) => Some(err),
            _ => None,
        }
    }
}

/// Serialize a structure into an owned byte vector.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    encode_into(value, &mut output)?;
    Ok(output)
}

/// Serialize a structure into the provided buffer, reusing the allocation.
pub fn encode_into<T: Serialize>(value: &T, output: &mut Vec<u8>) -> Result<()> {
    output.clear();
    {
        let mut serializer = Serializer { output };
        value.serialize(&mut serializer)?;
    }
    Ok(())
}

/// Deserialize a structure from the provided byte slice.
pub fn decode<T: DeserializeOwned>(input: &[u8]) -> Result<T> {
    let mut deserializer = Deserializer { input, position: 0 };
    let value = T::deserialize(&mut deserializer)?;
    if deserializer.position != deserializer.input.len() {
        return Err(Error::message("trailing bytes in binary payload"));
    }
    Ok(value)
}

struct Serializer<'a> {
    output: &'a mut Vec<u8>,
}

impl Serializer<'_> {
    fn write_u8(&mut self, value: u8) {
        self.output.push(value);
    }

    fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    fn write_u16(&mut self, value: u16) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u128(&mut self, value: u128) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_i64(&mut self, value: i64) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_i128(&mut self, value: i128) {
        self.output.extend_from_slice(&value.to_le_bytes());
    }

    fn write_f32(&mut self, value: f32) -> Result<()> {
        if value.is_finite() {
            self.output.extend_from_slice(&value.to_le_bytes());
            Ok(())
        } else {
            Err(Error::non_finite_float())
        }
    }

    fn write_f64(&mut self, value: f64) -> Result<()> {
        if value.is_finite() {
            self.output.extend_from_slice(&value.to_le_bytes());
            Ok(())
        } else {
            Err(Error::non_finite_float())
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_len(bytes.len())?;
        self.output.extend_from_slice(bytes);
        Ok(())
    }

    fn write_string(&mut self, value: &str) -> Result<()> {
        self.write_bytes(value.as_bytes())
    }

    fn write_len(&mut self, len: usize) -> Result<()> {
        let len = u64::try_from(len).map_err(|_| Error::length_overflow())?;
        self.write_u64(len);
        Ok(())
    }
}

impl<'a, 'b> ser::Serializer for &'a mut Serializer<'b> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = SeqSerializer<'a, 'b>;
    type SerializeTuple = TupleSerializer<'a, 'b>;
    type SerializeTupleStruct = TupleSerializer<'a, 'b>;
    type SerializeTupleVariant = TupleVariantSerializer<'a, 'b>;
    type SerializeMap = MapSerializer<'a, 'b>;
    type SerializeStruct = StructSerializer<'a, 'b>;
    type SerializeStructVariant = StructVariantSerializer<'a, 'b>;

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.write_bool(v);
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<()> {
        self.write_i64(v as i64);
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<()> {
        self.write_i64(v as i64);
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<()> {
        self.write_i64(v as i64);
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<()> {
        self.write_i64(v);
        Ok(())
    }

    fn serialize_i128(self, v: i128) -> Result<()> {
        self.write_i128(v);
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<()> {
        self.write_u8(v);
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<()> {
        self.write_u16(v);
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<()> {
        self.write_u32(v);
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<()> {
        self.write_u64(v);
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<()> {
        self.write_u128(v);
        Ok(())
    }

    fn serialize_usize(self, v: usize) -> Result<()> {
        // Serialize as u64 for platform independence
        self.serialize_u64(v as u64)
    }

    fn serialize_isize(self, v: isize) -> Result<()> {
        // Serialize as i64 for platform independence
        self.serialize_i64(v as i64)
    }

    fn serialize_f32(self, v: f32) -> Result<()> {
        self.write_f32(v)
    }

    fn serialize_f64(self, v: f64) -> Result<()> {
        self.write_f64(v)
    }

    fn serialize_char(self, v: char) -> Result<()> {
        self.write_u32(v as u32);
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        self.write_string(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.write_bytes(v)
    }

    fn serialize_none(self) -> Result<()> {
        self.write_bool(false);
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        self.write_bool(true);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<()> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<()> {
        self.write_u32(variant_index);
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<()> {
        self.write_u32(variant_index);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        SeqSerializer::new(self, len)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        TupleSerializer::new(self, len)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        TupleSerializer::new(self, len)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        TupleVariantSerializer::new(self, variant_index, len)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        MapSerializer::new(self, len)
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        StructSerializer::new(self, len)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        StructVariantSerializer::new(self, variant_index, len)
    }
}

struct SeqSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    len_pos: usize,
    count: u64,
}

impl<'a, 'b> SeqSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, len: Option<usize>) -> Result<Self> {
        let len_pos = serializer.output.len();
        serializer.write_u64(len.unwrap_or(0) as u64);
        Ok(Self {
            serializer,
            len_pos,
            count: 0,
        })
    }

    fn bump(&mut self) -> Result<()> {
        if self.count == u64::MAX {
            return Err(Error::length_overflow());
        }
        self.count += 1;
        Ok(())
    }

    fn finish(self) -> Result<()> {
        let bytes = self.count.to_le_bytes();
        self.serializer.output[self.len_pos..self.len_pos + 8].copy_from_slice(&bytes);
        Ok(())
    }
}

impl SerializeSeq for SeqSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        value.serialize(&mut *self.serializer)?;
        self.bump()
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl SerializeTuple for SeqSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for SeqSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeSeq::end(self)
    }
}

struct TupleSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    remaining: usize,
}

impl<'a, 'b> TupleSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, len: usize) -> Result<Self> {
        serializer.write_len(len)?;
        Ok(Self {
            serializer,
            remaining: len,
        })
    }
}

impl SerializeTuple for TupleSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if self.remaining == 0 {
            return Err(Error::message("too many tuple elements"));
        }
        value.serialize(&mut *self.serializer)?;
        self.remaining -= 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(Error::message("not enough tuple elements"))
        }
    }
}

impl SerializeTupleStruct for TupleSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        SerializeTuple::serialize_element(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeTuple::end(self)
    }
}

struct TupleVariantSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    remaining: usize,
}

impl<'a, 'b> TupleVariantSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, variant_index: u32, len: usize) -> Result<Self> {
        serializer.write_u32(variant_index);
        serializer.write_len(len)?;
        Ok(Self {
            serializer,
            remaining: len,
        })
    }
}

impl SerializeTupleVariant for TupleVariantSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if self.remaining == 0 {
            return Err(Error::message("too many tuple variant elements"));
        }
        value.serialize(&mut *self.serializer)?;
        self.remaining -= 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(Error::message("not enough tuple variant elements"))
        }
    }
}

struct MapSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    len_pos: usize,
    count: u64,
    awaiting_value: bool,
}

impl<'a, 'b> MapSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, len: Option<usize>) -> Result<Self> {
        let len_pos = serializer.output.len();
        serializer.write_u64(len.unwrap_or(0) as u64);
        Ok(Self {
            serializer,
            len_pos,
            count: 0,
            awaiting_value: false,
        })
    }

    fn finish(self) -> Result<()> {
        if self.awaiting_value {
            return Err(Error::message("map has dangling key"));
        }
        let bytes = self.count.to_le_bytes();
        self.serializer.output[self.len_pos..self.len_pos + 8].copy_from_slice(&bytes);
        Ok(())
    }
}

impl SerializeMap for MapSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        if self.awaiting_value {
            return Err(Error::message("serialize_key called twice"));
        }
        key.serialize(&mut *self.serializer)?;
        self.awaiting_value = true;
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if !self.awaiting_value {
            return Err(Error::message("serialize_value called before key"));
        }
        value.serialize(&mut *self.serializer)?;
        self.awaiting_value = false;
        if self.count == u64::MAX {
            return Err(Error::length_overflow());
        }
        self.count += 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

struct StructSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    remaining: usize,
}

impl<'a, 'b> StructSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, len: usize) -> Result<Self> {
        serializer.write_len(len)?;
        Ok(Self {
            serializer,
            remaining: len,
        })
    }
}

impl SerializeStruct for StructSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        if self.remaining == 0 {
            return Err(Error::message("too many struct fields"));
        }
        self.serializer.write_string(key)?;
        value.serialize(&mut *self.serializer)?;
        self.remaining -= 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(Error::message("not enough struct fields"))
        }
    }
}

struct StructVariantSerializer<'a, 'b> {
    serializer: &'a mut Serializer<'b>,
    remaining: usize,
}

impl<'a, 'b> StructVariantSerializer<'a, 'b> {
    fn new(serializer: &'a mut Serializer<'b>, variant_index: u32, len: usize) -> Result<Self> {
        serializer.write_u32(variant_index);
        serializer.write_len(len)?;
        Ok(Self {
            serializer,
            remaining: len,
        })
    }
}

impl SerializeStructVariant for StructVariantSerializer<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        if self.remaining == 0 {
            return Err(Error::message("too many struct variant fields"));
        }
        self.serializer.write_string(key)?;
        value.serialize(&mut *self.serializer)?;
        self.remaining -= 1;
        Ok(())
    }

    fn end(self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(Error::message("not enough struct variant fields"))
        }
    }
}

struct Deserializer<'de> {
    input: &'de [u8],
    position: usize,
}

impl<'de> Deserializer<'de> {
    fn read_exact(&mut self, len: usize) -> Result<&'de [u8]> {
        if self.position + len > self.input.len() {
            return Err(Error::unexpected_eof());
        }
        let slice = &self.input[self.position..self.position + len];
        self.position += len;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(Error::invalid_bool(other)),
        }
    }

    fn read_u16(&mut self) -> Result<u16> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(bytes))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_exact(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_exact(8)?);
        Ok(i64::from_le_bytes(bytes))
    }

    fn read_u128(&mut self) -> Result<u128> {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(self.read_exact(16)?);
        Ok(u128::from_le_bytes(bytes))
    }

    fn read_i128(&mut self) -> Result<i128> {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(self.read_exact(16)?);
        Ok(i128::from_le_bytes(bytes))
    }

    fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read_exact(4)?.try_into().unwrap()))
    }

    fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.read_exact(8)?.try_into().unwrap()))
    }

    fn read_len(&mut self) -> Result<usize> {
        let len = self.read_u64()?;
        usize::try_from(len).map_err(|_| Error::length_overflow())
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_len()?;
        Ok(self.read_exact(len)?.to_vec())
    }

    fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(Error::invalid_utf8)
    }
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_bool(self.read_bool()?)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i8(self.read_i64()? as i8)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i16(self.read_i64()? as i16)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i32(self.read_i64()? as i32)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i64(self.read_i64()?)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i128(self.read_i128()?)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u8(self.read_u8()?)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u16(self.read_u16()?)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u32(self.read_u32()?)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u64(self.read_u64()?)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u128(self.read_u128()?)
    }

    fn deserialize_usize<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self.read_u64()?;
        // Validate the value fits in usize on this platform
        if value > usize::MAX as u64 {
            return Err(Error::message(
                "u64 value too large for usize on this platform",
            ));
        }
        visitor.visit_usize(value as usize)
    }

    fn deserialize_isize<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = self.read_i64()?;
        // Validate the value fits in isize on this platform
        if value > isize::MAX as i64 || value < isize::MIN as i64 {
            return Err(Error::message(
                "i64 value out of range for isize on this platform",
            ));
        }
        visitor.visit_isize(value as isize)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f32(self.read_f32()?)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f64(self.read_f64()?)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let ch = char::from_u32(self.read_u32()?).ok_or_else(|| Error::message("invalid char"))?;
        visitor.visit_char(ch)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let string = self.read_string()?;
        visitor.visit_string(string)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let bytes = self.read_bytes()?;
        visitor.visit_byte_buf(bytes)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.read_bool()? {
            visitor.visit_some(self)
        } else {
            visitor.visit_none()
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let len = self.read_len()?;
        visitor.visit_seq(SeqAccessDeserializer {
            de: self,
            remaining: len,
        })
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let actual = self.read_len()?;
        if actual != len {
            return Err(Error::message("tuple length mismatch"));
        }
        visitor.visit_seq(SeqAccessDeserializer {
            de: self,
            remaining: len,
        })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let len = self.read_len()?;
        visitor.visit_map(MapAccessDeserializer {
            de: self,
            remaining: len,
            awaiting_value: false,
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let len = self.read_len()?;
        visitor.visit_map(StructAccessDeserializer {
            de: self,
            remaining: len,
            pending_key: None,
        })
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let index = self.read_u32()?;
        visitor.visit_enum(EnumDeserializer { de: self, index })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct SeqAccessDeserializer<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for SeqAccessDeserializer<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }
}

struct MapAccessDeserializer<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    remaining: usize,
    awaiting_value: bool,
}

impl<'de> MapAccess<'de> for MapAccessDeserializer<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        if self.awaiting_value {
            return Err(Error::message("value missing for previous key"));
        }
        self.awaiting_value = true;
        seed.deserialize(&mut *self.de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        if !self.awaiting_value {
            return Err(Error::message("serialize_value called before key"));
        }
        self.awaiting_value = false;
        self.remaining -= 1;
        seed.deserialize(&mut *self.de)
    }
}

struct StructAccessDeserializer<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    remaining: usize,
    pending_key: Option<String>,
}

impl<'de> MapAccess<'de> for StructAccessDeserializer<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        let key = self.de.read_string()?;
        self.pending_key = Some(key.clone());
        let de = StringDeserializer::new(key);
        seed.deserialize(de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        if self.pending_key.is_none() {
            return Err(Error::message("value requested before key"));
        }
        self.pending_key = None;
        self.remaining -= 1;
        seed.deserialize(&mut *self.de)
    }
}

struct EnumDeserializer<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    index: u32,
}

impl<'de, 'a> EnumAccess<'de> for EnumDeserializer<'a, 'de> {
    type Error = Error;
    type Variant = VariantDeserializer<'a, 'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        let value = seed.deserialize(self.index.into_deserializer())?;
        Ok((value, VariantDeserializer { de: self.de }))
    }
}

struct VariantDeserializer<'a, 'de> {
    de: &'a mut Deserializer<'de>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'_, 'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Ok(())
    }

    fn newtype_variant<T>(self) -> Result<T>
    where
        T: de::Deserialize<'de>,
    {
        T::deserialize(self.de)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self.de, len, visitor)
    }

    fn struct_variant<V>(self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_struct(self.de, "", &[], visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ser::SerializeStruct;
    use crate::{de, ser, Deserialize, Serialize};

    use core::{fmt, result::Result as StdResult};

    #[derive(Debug, PartialEq)]
    struct Example {
        id: u64,
        label: String,
        flags: Vec<bool>,
        tuple: (u32, u32),
    }

    impl Serialize for Example {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Example", 4)?;
            state.serialize_field("id", &self.id)?;
            state.serialize_field("label", &self.label)?;
            state.serialize_field("flags", &self.flags)?;
            state.serialize_field("tuple", &self.tuple)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for Example {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            enum Field {
                Id,
                Label,
                Flags,
                Tuple,
            }

            struct FieldVisitor;

            impl<'de> de::Visitor<'de> for FieldVisitor {
                type Value = Field;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("`id`, `label`, `flags`, or `tuple`")
                }

                fn visit_str<E>(self, value: &str) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        "id" => Ok(Field::Id),
                        "label" => Ok(Field::Label),
                        "flags" => Ok(Field::Flags),
                        "tuple" => Ok(Field::Tuple),
                        other => Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                fn visit_bytes<E>(self, value: &[u8]) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    match value {
                        b"id" => Ok(Field::Id),
                        b"label" => Ok(Field::Label),
                        b"flags" => Ok(Field::Flags),
                        b"tuple" => Ok(Field::Tuple),
                        other => {
                            let text = core::str::from_utf8(other).unwrap_or("");
                            Err(de::Error::unknown_field(text, FIELDS))
                        }
                    }
                }

                fn visit_string<E>(self, value: String) -> StdResult<Field, E>
                where
                    E: de::Error,
                {
                    self.visit_str(&value)
                }
            }

            impl<'de> Deserialize<'de> for Field {
                fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
                where
                    D: de::Deserializer<'de>,
                {
                    deserializer.deserialize_identifier(FieldVisitor)
                }
            }

            struct ExampleVisitor;

            impl<'de> de::Visitor<'de> for ExampleVisitor {
                type Value = Example;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("Example struct")
                }

                fn visit_map<M>(self, mut map: M) -> StdResult<Example, M::Error>
                where
                    M: de::MapAccess<'de>,
                {
                    let mut id = None;
                    let mut label = None;
                    let mut flags = None;
                    let mut tuple = None;
                    while let Some(field) = map.next_key::<Field>()? {
                        match field {
                            Field::Id => {
                                if id.is_some() {
                                    return Err(de::Error::duplicate_field("id"));
                                }
                                id = Some(map.next_value()?);
                            }
                            Field::Label => {
                                if label.is_some() {
                                    return Err(de::Error::duplicate_field("label"));
                                }
                                label = Some(map.next_value()?);
                            }
                            Field::Flags => {
                                if flags.is_some() {
                                    return Err(de::Error::duplicate_field("flags"));
                                }
                                flags = Some(map.next_value()?);
                            }
                            Field::Tuple => {
                                if tuple.is_some() {
                                    return Err(de::Error::duplicate_field("tuple"));
                                }
                                tuple = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(Example {
                        id: id.ok_or_else(|| de::Error::missing_field("id"))?,
                        label: label.ok_or_else(|| de::Error::missing_field("label"))?,
                        flags: flags.ok_or_else(|| de::Error::missing_field("flags"))?,
                        tuple: tuple.ok_or_else(|| de::Error::missing_field("tuple"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["id", "label", "flags", "tuple"];
            deserializer.deserialize_struct("Example", FIELDS, ExampleVisitor)
        }
    }

    #[test]
    fn binary_roundtrip() {
        let example = Example {
            id: 7,
            label: "example".to_string(),
            flags: vec![true, false, true],
            tuple: (3, 4),
        };
        let encoded = encode(&example).expect("encode");
        let decoded: Example = decode(&encoded).expect("decode");
        assert_eq!(decoded, example);
    }
}
