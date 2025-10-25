#![forbid(unsafe_code)]

use std::convert::TryFrom;
use std::fmt;

/// Error returned by the manual binary cursor helpers.
#[derive(Debug)]
pub enum CursorError {
    /// Encountered the end of input before the requested number of bytes.
    UnexpectedEof,
    /// Boolean discriminant outside the supported range of `0` or `1`.
    InvalidBool(u8),
    /// Length prefix could not fit into the host pointer size.
    LengthOverflow(u64),
    /// Encountered invalid UTF-8 data while decoding a string value.
    InvalidUtf8(std::string::FromUtf8Error),
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CursorError::UnexpectedEof => write!(f, "unexpected end of binary payload"),
            CursorError::InvalidBool(value) => {
                write!(f, "invalid boolean discriminant: {value}")
            }
            CursorError::LengthOverflow(value) => {
                write!(f, "length prefix {value} exceeds usize::MAX")
            }
            CursorError::InvalidUtf8(err) => write!(f, "invalid utf-8 sequence: {err}"),
        }
    }
}

impl std::error::Error for CursorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CursorError::InvalidUtf8(err) => Some(err),
            _ => None,
        }
    }
}

/// Minimal writer that mirrors the struct encoding format previously produced by
/// `binary::encode`.
#[derive(Debug, Default)]
pub struct Writer {
    buffer: Vec<u8>,
}

impl Writer {
    /// Create a new writer.
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Create a writer with the provided capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Return a reference to the internal buffer.
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer
    }

    /// Consume the writer returning the encoded bytes.
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }

    /// Write a single byte to the buffer.
    pub fn write_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    /// Write a boolean value using the historical encoding (`0`/`1`).
    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    /// Write a little-endian `u16`.
    pub fn write_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a little-endian `u32`.
    pub fn write_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a little-endian `u64`.
    pub fn write_u64(&mut self, value: u64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a little-endian `u128`.
    pub fn write_u128(&mut self, value: u128) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a little-endian `i64`.
    pub fn write_i64(&mut self, value: i64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a little-endian `i128`.
    pub fn write_i128(&mut self, value: i128) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write an IEEE-754 little-endian `f32`.
    pub fn write_f32(&mut self, value: f32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write an IEEE-754 little-endian `f64`.
    pub fn write_f64(&mut self, value: f64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write the provided byte slice preceded by its length.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_len(bytes.len());
        self.buffer.extend_from_slice(bytes);
    }

    /// Write the provided string preceded by its UTF-8 byte length.
    pub fn write_string(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    /// Internal helper that mirrors the struct encoder length handling.
    fn write_len(&mut self, len: usize) {
        self.write_u64(len as u64);
    }

    /// Write a vector of values.
    pub fn write_vec_with<T, F>(&mut self, values: &[T], mut write: F)
    where
        F: FnMut(&mut Writer, &T),
    {
        let len = u64::try_from(values.len()).expect("vector length exceeds u64::MAX");
        self.write_u64(len);
        for value in values {
            write(self, value);
        }
    }

    /// Write an optional value.
    pub fn write_option_with<T: ?Sized, F>(&mut self, value: Option<&T>, mut write: F)
    where
        F: FnMut(&mut Writer, &T),
    {
        match value {
            Some(inner) => {
                self.write_bool(true);
                write(self, inner);
            }
            None => {
                self.write_bool(false);
            }
        }
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Begin writing a struct encoded as a field-count-prefixed map.
    pub fn write_struct<F>(&mut self, build: F)
    where
        F: FnOnce(&mut StructWriter),
    {
        let mut struct_writer = StructWriter::default();
        build(&mut struct_writer);
        self.write_u64(struct_writer.count);
        self.extend_from_slice(struct_writer.buffer.as_slice());
    }
}

/// Reader counterpart to [`Writer`] that understands the legacy binary layout.
#[derive(Debug)]
pub struct Reader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    /// Create a new reader over the provided byte slice.
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    /// Number of unread bytes remaining in the buffer.
    pub fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.position)
    }

    /// Read exactly `len` bytes from the cursor.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CursorError> {
        if self
            .position
            .checked_add(len)
            .is_none_or(|end| end > self.input.len())
        {
            return Err(CursorError::UnexpectedEof);
        }
        let slice = &self.input[self.position..self.position + len];
        self.position += len;
        Ok(slice)
    }

    /// Read a single byte.
    pub fn read_u8(&mut self) -> Result<u8, CursorError> {
        Ok(self.read_exact(1)?[0])
    }

    /// Read a boolean value.
    pub fn read_bool(&mut self) -> Result<bool, CursorError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CursorError::InvalidBool(other)),
        }
    }

    /// Read a little-endian `u16`.
    pub fn read_u16(&mut self) -> Result<u16, CursorError> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(bytes))
    }

    /// Read a little-endian `u32`.
    pub fn read_u32(&mut self) -> Result<u32, CursorError> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    /// Read a little-endian `u64`.
    pub fn read_u64(&mut self) -> Result<u64, CursorError> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_exact(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    /// Read a little-endian `u128`.
    pub fn read_u128(&mut self) -> Result<u128, CursorError> {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(self.read_exact(16)?);
        Ok(u128::from_le_bytes(bytes))
    }

    /// Read a little-endian `i64`.
    pub fn read_i64(&mut self) -> Result<i64, CursorError> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_exact(8)?);
        Ok(i64::from_le_bytes(bytes))
    }

    /// Read a little-endian `i128`.
    pub fn read_i128(&mut self) -> Result<i128, CursorError> {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(self.read_exact(16)?);
        Ok(i128::from_le_bytes(bytes))
    }

    /// Read an IEEE-754 `f32` encoded in little-endian order.
    pub fn read_f32(&mut self) -> Result<f32, CursorError> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.read_exact(4)?);
        Ok(f32::from_le_bytes(bytes))
    }

    /// Read an IEEE-754 `f64` encoded in little-endian order.
    pub fn read_f64(&mut self) -> Result<f64, CursorError> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_exact(8)?);
        Ok(f64::from_le_bytes(bytes))
    }

    /// Read a length-prefixed byte vector.
    pub fn read_bytes(&mut self) -> Result<Vec<u8>, CursorError> {
        let len = self.read_u64()?;
        let len = usize::try_from(len).map_err(|_| CursorError::LengthOverflow(len))?;
        Ok(self.read_exact(len)?.to_vec())
    }

    /// Read a UTF-8 string using the legacy length-prefixed encoding.
    pub fn read_string(&mut self) -> Result<String, CursorError> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(CursorError::InvalidUtf8)
    }

    /// Read an optional value encoded with a boolean discriminant.
    pub fn read_option_with<T, F, E>(&mut self, mut read: F) -> Result<Option<T>, E>
    where
        F: FnMut(&mut Reader<'a>) -> Result<T, E>,
        E: From<CursorError>,
    {
        if self.read_bool().map_err(E::from)? {
            read(self).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Read a vector of values.
    pub fn read_vec_with<T, F, E>(&mut self, mut read: F) -> Result<Vec<T>, E>
    where
        F: FnMut(&mut Reader<'a>) -> Result<T, E>,
        E: From<CursorError>,
    {
        let len = self.read_u64().map_err(E::from)?;
        let len = usize::try_from(len).map_err(|_| E::from(CursorError::LengthOverflow(len)))?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(read(self)?);
        }
        Ok(values)
    }

    /// Read a struct encoded as a sequence of key/value pairs, returning the field count.
    pub fn read_struct_with<F, E>(&mut self, mut visit: F) -> Result<u64, E>
    where
        F: FnMut(&str, &mut Reader<'a>) -> Result<(), E>,
        E: From<CursorError>,
    {
        let field_count = self.read_u64().map_err(E::from)?;
        for _ in 0..field_count {
            let key = self.read_string().map_err(E::from)?;
            visit(&key, self)?;
        }
        Ok(field_count)
    }
}

/// Helper used to build struct encodings without repeating boilerplate.
#[derive(Debug, Default)]
pub struct StructWriter {
    buffer: Writer,
    count: u64,
}

impl StructWriter {
    /// Write a value for the provided key using the supplied closure.
    pub fn field_with<F>(&mut self, key: &str, mut write: F)
    where
        F: FnMut(&mut Writer),
    {
        self.count = self
            .count
            .checked_add(1)
            .expect("struct field count overflowed u64::MAX");
        self.buffer.write_string(key);
        write(&mut self.buffer);
    }

    /// Write a string field.
    pub fn field_string(&mut self, key: &str, value: &str) {
        self.field_with(key, |writer| writer.write_string(value));
    }

    /// Write a byte slice field.
    pub fn field_bytes(&mut self, key: &str, value: &[u8]) {
        self.field_with(key, |writer| writer.write_bytes(value));
    }

    /// Write a `u8` field.
    pub fn field_u8(&mut self, key: &str, value: u8) {
        self.field_with(key, |writer| writer.write_u8(value));
    }

    /// Write a boolean field.
    pub fn field_bool(&mut self, key: &str, value: bool) {
        self.field_with(key, |writer| writer.write_bool(value));
    }

    /// Write a `u64` field.
    pub fn field_u64(&mut self, key: &str, value: u64) {
        self.field_with(key, |writer| writer.write_u64(value));
    }

    /// Write a `u32` field.
    pub fn field_u32(&mut self, key: &str, value: u32) {
        self.field_with(key, |writer| writer.write_u32(value));
    }

    /// Write an `i64` field.
    pub fn field_i64(&mut self, key: &str, value: i64) {
        self.field_with(key, |writer| writer.write_i64(value));
    }

    /// Write an `f64` field.
    pub fn field_f64(&mut self, key: &str, value: f64) {
        self.field_with(key, |writer| writer.write_f64(value));
    }

    /// Write an `f32` field.
    pub fn field_f32(&mut self, key: &str, value: f32) {
        self.field_with(key, |writer| writer.write_f32(value));
    }

    /// Write an optional `u64` field.
    pub fn field_option_u64(&mut self, key: &str, value: Option<u64>) {
        self.field_with(key, |writer| {
            writer.write_option_with(value.as_ref(), |w, inner| w.write_u64(*inner))
        });
    }

    /// Write an optional string field.
    pub fn field_option_string(&mut self, key: &str, value: Option<&str>) {
        self.field_with(key, |writer| {
            writer.write_option_with(value, |w, inner| w.write_string(inner))
        });
    }

    /// Write a vector field using the provided element writer.
    pub fn field_vec_with<T, F>(&mut self, key: &str, values: &[T], mut write: F)
    where
        F: FnMut(&mut Writer, &T),
    {
        self.field_with(key, |writer| {
            writer.write_vec_with(values, |w, value| write(w, value))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, Writer};
    use crate::ser::SerializeStruct;
    use crate::{binary, ser, Serialize};
    use core::result::Result as StdResult;

    struct Sample<'a> {
        domain: &'a str,
        provider_id: &'a str,
        bytes_served: u64,
        ts: u64,
        dynamic: bool,
        allowed: bool,
    }

    struct FloatSample {
        ratio: f64,
        weight: f32,
    }

    impl<'a> Serialize for Sample<'a> {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("Sample", 6)?;
            state.serialize_field("domain", &self.domain)?;
            state.serialize_field("provider_id", &self.provider_id)?;
            state.serialize_field("bytes_served", &self.bytes_served)?;
            state.serialize_field("ts", &self.ts)?;
            state.serialize_field("dynamic", &self.dynamic)?;
            state.serialize_field("allowed", &self.allowed)?;
            state.end()
        }
    }

    impl Serialize for FloatSample {
        fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            let mut state = serializer.serialize_struct("FloatSample", 2)?;
            state.serialize_field("ratio", &self.ratio)?;
            state.serialize_field("weight", &self.weight)?;
            state.end()
        }
    }

    #[test]
    fn writer_matches_legacy_encoding() {
        let sample = Sample {
            domain: "example.com",
            provider_id: "provider-123",
            bytes_served: 512,
            ts: 42,
            dynamic: true,
            allowed: false,
        };

        let legacy = binary::encode(&sample).expect("legacy encode");

        let mut writer = Writer::new();
        writer.write_u64(6);
        writer.write_string("domain");
        writer.write_string(sample.domain);
        writer.write_string("provider_id");
        writer.write_string(sample.provider_id);
        writer.write_string("bytes_served");
        writer.write_u64(sample.bytes_served);
        writer.write_string("ts");
        writer.write_u64(sample.ts);
        writer.write_string("dynamic");
        writer.write_bool(sample.dynamic);
        writer.write_string("allowed");
        writer.write_bool(sample.allowed);

        assert_eq!(legacy, writer.finish());
    }

    #[test]
    fn reader_round_trips_values() {
        let sample = Sample {
            domain: "example.com",
            provider_id: "provider-123",
            bytes_served: 512,
            ts: 42,
            dynamic: true,
            allowed: false,
        };
        let bytes = binary::encode(&sample).expect("legacy encode");

        let mut reader = Reader::new(&bytes);
        let field_count = reader.read_u64().expect("field count");
        assert_eq!(field_count, 6);

        let domain_key = reader.read_string().expect("domain key");
        assert_eq!(domain_key, "domain");
        let domain_value = reader.read_string().expect("domain value");
        assert_eq!(domain_value, sample.domain);

        let provider_key = reader.read_string().expect("provider key");
        assert_eq!(provider_key, "provider_id");
        let provider_value = reader.read_string().expect("provider value");
        assert_eq!(provider_value, sample.provider_id);

        let bytes_key = reader.read_string().expect("bytes key");
        assert_eq!(bytes_key, "bytes_served");
        let bytes_value = reader.read_u64().expect("bytes value");
        assert_eq!(bytes_value, sample.bytes_served);

        let ts_key = reader.read_string().expect("ts key");
        assert_eq!(ts_key, "ts");
        let ts_value = reader.read_u64().expect("ts value");
        assert_eq!(ts_value, sample.ts);

        let dynamic_key = reader.read_string().expect("dynamic key");
        assert_eq!(dynamic_key, "dynamic");
        let dynamic_value = reader.read_bool().expect("dynamic value");
        assert_eq!(dynamic_value, sample.dynamic);

        let allowed_key = reader.read_string().expect("allowed key");
        assert_eq!(allowed_key, "allowed");
        let allowed_value = reader.read_bool().expect("allowed value");
        assert_eq!(allowed_value, sample.allowed);

        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn float_helpers_match_legacy_encoding() {
        let sample = FloatSample {
            ratio: 3.1415,
            weight: 0.625,
        };

        let legacy = binary::encode(&sample).expect("legacy encode");

        let mut writer = Writer::new();
        writer.write_u64(2);
        writer.write_string("ratio");
        writer.write_f64(sample.ratio);
        writer.write_string("weight");
        writer.write_f32(sample.weight);
        assert_eq!(legacy, writer.finish());

        let mut reader = Reader::new(&legacy);
        let fields = reader.read_u64().expect("field count");
        assert_eq!(fields, 2);
        let key_ratio = reader.read_string().expect("ratio key");
        assert_eq!(key_ratio, "ratio");
        let ratio = reader.read_f64().expect("ratio value");
        assert!((ratio - sample.ratio).abs() < f64::EPSILON);
        let key_weight = reader.read_string().expect("weight key");
        assert_eq!(key_weight, "weight");
        let weight = reader.read_f32().expect("weight value");
        assert!((weight - sample.weight).abs() < f32::EPSILON);
        assert_eq!(reader.remaining(), 0);
    }
}
