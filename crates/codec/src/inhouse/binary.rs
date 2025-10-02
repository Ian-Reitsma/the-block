use core::convert::TryInto;

use super::{Error, Result};

/// Trait implemented by types that can be serialized with the in-house binary encoder.
pub trait BinaryEncoder {
    fn encode_binary(&self, writer: &mut BinaryWriter);
}

/// Trait implemented by types that can be deserialized from the in-house binary format.
pub trait BinaryDecode: Sized {
    fn decode_binary(input: &mut &[u8]) -> Result<Self>;
}

/// Write-only helper that emits deterministic little-endian payloads.
#[derive(Default, Debug)]
pub struct BinaryWriter {
    buffer: Vec<u8>,
}

impl BinaryWriter {
    pub fn write_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    pub fn write_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u32(bytes.len() as u32);
        self.buffer.extend_from_slice(bytes);
    }

    pub fn write_string(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    pub fn write_vec<T>(&mut self, values: &[T])
    where
        T: BinaryEncoder,
    {
        self.write_u32(values.len() as u32);
        for value in values {
            value.encode_binary(self);
        }
    }

    pub fn write_option<T>(&mut self, value: &Option<T>)
    where
        T: BinaryEncoder,
    {
        match value {
            Some(inner) => {
                self.write_bool(true);
                inner.encode_binary(self);
            }
            None => self.write_bool(false),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }

    fn read_bytes_internal<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
        if input.len() < len {
            return Err(Error::UnexpectedEof);
        }
        let (prefix, rest) = input.split_at(len);
        *input = rest;
        Ok(prefix)
    }

    pub fn read_u8(input: &mut &[u8]) -> Result<u8> {
        if input.is_empty() {
            return Err(Error::UnexpectedEof);
        }
        let (value, rest) = input.split_first().unwrap();
        *input = rest;
        Ok(*value)
    }

    pub fn read_bool(input: &mut &[u8]) -> Result<bool> {
        match Self::read_u8(input)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(Error::InvalidBool(other)),
        }
    }

    pub fn read_u16(input: &mut &[u8]) -> Result<u16> {
        let bytes = Self::read_bytes_internal(input, 2)?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u32(input: &mut &[u8]) -> Result<u32> {
        let bytes = Self::read_bytes_internal(input, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u64(input: &mut &[u8]) -> Result<u64> {
        let bytes = Self::read_bytes_internal(input, 8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_bytes(input: &mut &[u8]) -> Result<Vec<u8>> {
        let len = Self::read_u32(input)? as usize;
        let bytes = Self::read_bytes_internal(input, len)?;
        Ok(bytes.to_vec())
    }

    pub fn read_string(input: &mut &[u8]) -> Result<String> {
        let bytes = Self::read_bytes(input)?;
        String::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
    }

    pub fn read_vec<T>(input: &mut &[u8]) -> Result<Vec<T>>
    where
        T: BinaryDecode,
    {
        let len = Self::read_u32(input)? as usize;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(T::decode_binary(input)?);
        }
        Ok(values)
    }

    pub fn read_option<T>(input: &mut &[u8]) -> Result<Option<T>>
    where
        T: BinaryDecode,
    {
        if Self::read_bool(input)? {
            Ok(Some(T::decode_binary(input)?))
        } else {
            Ok(None)
        }
    }
}

impl BinaryEncoder for u8 {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_u8(*self);
    }
}

impl BinaryDecode for u8 {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_u8(input)
    }
}

impl BinaryEncoder for bool {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_bool(*self);
    }
}

impl BinaryDecode for bool {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_bool(input)
    }
}

impl BinaryEncoder for u16 {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_u16(*self);
    }
}

impl BinaryDecode for u16 {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_u16(input)
    }
}

impl BinaryEncoder for u32 {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_u32(*self);
    }
}

impl BinaryDecode for u32 {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_u32(input)
    }
}

impl BinaryEncoder for u64 {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_u64(*self);
    }
}

impl BinaryDecode for u64 {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_u64(input)
    }
}

impl BinaryEncoder for String {
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_string(self);
    }
}

impl BinaryDecode for String {
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_string(input)
    }
}

impl<T> BinaryEncoder for Vec<T>
where
    T: BinaryEncoder,
{
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_vec(self);
    }
}

impl<T> BinaryDecode for Vec<T>
where
    T: BinaryDecode,
{
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_vec(input)
    }
}

impl<T> BinaryEncoder for Option<T>
where
    T: BinaryEncoder,
{
    fn encode_binary(&self, writer: &mut BinaryWriter) {
        writer.write_option(self);
    }
}

impl<T> BinaryDecode for Option<T>
where
    T: BinaryDecode,
{
    fn decode_binary(input: &mut &[u8]) -> Result<Self> {
        BinaryWriter::read_option(input)
    }
}
