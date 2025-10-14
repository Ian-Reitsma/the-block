use std::fmt;

use foundation_serialization::binary_cursor::{CursorError, Reader};

/// Result alias for binary struct decoding helpers.
pub type Result<T> = std::result::Result<T, DecodeError>;

/// Error returned by the manual binary struct decoders.
#[derive(Debug)]
pub enum DecodeError {
    /// Cursor-level failure while reading the payload.
    Cursor(CursorError),
    /// Encountered an unexpected number of fields in the struct.
    InvalidFieldCount { expected: u64, actual: u64 },
    /// Encountered an unknown field name.
    UnknownField(String),
    /// Encountered the same field multiple times.
    DuplicateField(&'static str),
    /// Required field missing from the payload.
    MissingField(&'static str),
    /// Trailing bytes were present after decoding finished.
    TrailingBytes(usize),
    /// Invalid enum discriminant encountered during decoding.
    InvalidEnumDiscriminant { ty: &'static str, value: u32 },
    /// Field contained an invalid value that could not be parsed.
    InvalidFieldValue { field: &'static str, reason: String },
}

impl From<CursorError> for DecodeError {
    fn from(err: CursorError) -> Self {
        Self::Cursor(err)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Cursor(err) => write!(f, "cursor error: {err}"),
            DecodeError::InvalidFieldCount { expected, actual } => {
                write!(f, "invalid field count: expected {expected} got {actual}")
            }
            DecodeError::UnknownField(name) => {
                write!(f, "unknown field '{name}' in binary payload")
            }
            DecodeError::DuplicateField(name) => {
                write!(f, "duplicate '{name}' field encountered")
            }
            DecodeError::MissingField(name) => {
                write!(f, "missing required '{name}' field")
            }
            DecodeError::TrailingBytes(count) => {
                write!(f, "{count} trailing byte(s) remaining after decode")
            }
            DecodeError::InvalidEnumDiscriminant { ty, value } => {
                write!(f, "invalid {ty} discriminant: {value}")
            }
            DecodeError::InvalidFieldValue { field, reason } => {
                write!(f, "invalid value for '{field}': {reason}")
            }
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Cursor(err) => Some(err),
            _ => None,
        }
    }
}

/// Assign `value` to `slot` ensuring the field is only observed once.
pub fn assign_once<T>(slot: &mut Option<T>, value: T, name: &'static str) -> Result<()> {
    if slot.is_some() {
        return Err(DecodeError::DuplicateField(name));
    }
    *slot = Some(value);
    Ok(())
}

/// Ensure there are no trailing bytes after decoding has finished.
pub fn ensure_exhausted(reader: &Reader<'_>) -> Result<()> {
    let remaining = reader.remaining();
    if remaining == 0 {
        Ok(())
    } else {
        Err(DecodeError::TrailingBytes(remaining))
    }
}

/// Decode a struct encoded via [`Writer::write_struct`](foundation_serialization::binary_cursor::Writer::write_struct).
pub fn decode_struct<'a, F>(
    reader: &mut Reader<'a>,
    expected_fields: Option<u64>,
    mut visit: F,
) -> Result<()>
where
    F: FnMut(&str, &mut Reader<'a>) -> Result<()>,
{
    let field_count = reader.read_struct_with(|key, reader| visit(key, reader))?;
    if let Some(expected) = expected_fields {
        if field_count != expected {
            return Err(DecodeError::InvalidFieldCount {
                expected,
                actual: field_count,
            });
        }
    }
    Ok(())
}
