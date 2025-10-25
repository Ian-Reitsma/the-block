#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::fmt;

/// Convenient result alias for the base64 helpers.
pub type Result<T> = std::result::Result<T, Error>;

/// Error describing why base64 processing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ErrorKind {
    Byte(u8),
    Length,
    Padding,
}

impl Error {
    fn invalid_byte(byte: u8, index: usize) -> Self {
        Self {
            kind: ErrorKind::Byte(byte),
            index: Some(index),
        }
    }

    fn invalid_length() -> Self {
        Self {
            kind: ErrorKind::Length,
            index: None,
        }
    }

    fn invalid_padding() -> Self {
        Self {
            kind: ErrorKind::Padding,
            index: None,
        }
    }

    /// Returns `true` when the error was triggered by an invalid byte.
    pub fn is_invalid_byte(&self) -> bool {
        matches!(self.kind, ErrorKind::Byte(_))
    }

    /// Returns `true` when the failure stems from an invalid length.
    pub fn is_invalid_length(&self) -> bool {
        matches!(self.kind, ErrorKind::Length)
    }

    /// Returns `true` when the error was caused by malformed padding.
    pub fn is_invalid_padding(&self) -> bool {
        matches!(self.kind, ErrorKind::Padding)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Byte(byte) => match self.index {
                Some(idx) => write!(f, "invalid base64 byte 0x{byte:02x} at index {idx}"),
                None => write!(f, "invalid base64 byte 0x{byte:02x}"),
            },
            ErrorKind::Length => write!(f, "invalid base64 length"),
            ErrorKind::Padding => write!(f, "invalid base64 padding"),
        }
    }
}

impl std::error::Error for Error {}

const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encode bytes with the standard base64 alphabet, including padding.
pub fn encode_standard(input: &[u8]) -> String {
    encode_internal(input, STANDARD_ALPHABET, true)
}

/// Decode a standard base64 string that may contain padding.
pub fn decode_standard(input: &str) -> Result<Vec<u8>> {
    if input.len() % 4 != 0 {
        return Err(Error::invalid_length());
    }
    decode_internal(input.as_bytes(), STANDARD_ALPHABET, true)
}

/// Encode bytes with the URL-safe alphabet without padding.
pub fn encode_url_no_pad(input: &[u8]) -> String {
    encode_internal(input, URL_SAFE_ALPHABET, false)
}

/// Decode a URL-safe, no-padding string.
pub fn decode_url_no_pad(input: &str) -> Result<Vec<u8>> {
    if input.contains('=') {
        return Err(Error::invalid_padding());
    }
    if input.len() % 4 == 1 {
        return Err(Error::invalid_length());
    }

    let padded = if input.len() % 4 == 0 {
        Cow::Borrowed(input)
    } else {
        let mut owned = String::with_capacity(input.len() + (4 - input.len() % 4));
        owned.push_str(input);
        for _ in 0..(4 - input.len() % 4) {
            owned.push('=');
        }
        Cow::Owned(owned)
    };

    decode_internal(padded.as_bytes(), URL_SAFE_ALPHABET, true)
}

fn encode_internal(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    if input.is_empty() {
        return String::new();
    }

    let mut output = Vec::with_capacity(input.len().div_ceil(3) * 4);
    let mut index = 0usize;

    while index + 3 <= input.len() {
        let chunk = &input[index..index + 3];
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        output.push(alphabet[((n >> 18) & 0x3f) as usize]);
        output.push(alphabet[((n >> 12) & 0x3f) as usize]);
        output.push(alphabet[((n >> 6) & 0x3f) as usize]);
        output.push(alphabet[(n & 0x3f) as usize]);
        index += 3;
    }

    let remainder = input.len() - index;
    if remainder == 1 {
        let n = (input[index] as u32) << 16;
        output.push(alphabet[((n >> 18) & 0x3f) as usize]);
        output.push(alphabet[((n >> 12) & 0x3f) as usize]);
        if pad {
            output.push(b'=');
            output.push(b'=');
        }
    } else if remainder == 2 {
        let n = ((input[index] as u32) << 16) | ((input[index + 1] as u32) << 8);
        output.push(alphabet[((n >> 18) & 0x3f) as usize]);
        output.push(alphabet[((n >> 12) & 0x3f) as usize]);
        output.push(alphabet[((n >> 6) & 0x3f) as usize]);
        if pad {
            output.push(b'=');
        }
    }

    String::from_utf8(output).expect("base64 output is always valid UTF-8")
}

fn decode_internal(input: &[u8], alphabet: &[u8; 64], expect_padding: bool) -> Result<Vec<u8>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    if input.len() % 4 != 0 {
        return Err(Error::invalid_length());
    }

    let mut table = [-1i16; 256];
    for (i, &byte) in alphabet.iter().enumerate() {
        table[byte as usize] = i as i16;
    }

    let total_chunks = input.len() / 4;
    let mut output = Vec::with_capacity(total_chunks * 3);

    for (chunk_index, chunk) in input.chunks_exact(4).enumerate() {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        let mut effective_len = 4usize;

        for i in 0..4 {
            let ch = chunk[i];
            if ch == b'=' {
                if !expect_padding {
                    return Err(Error::invalid_padding());
                }
                if i < 2 {
                    return Err(Error::invalid_padding());
                }
                effective_len = i;
                padding = 4 - i;
                for &byte in &chunk[i..] {
                    if byte != b'=' {
                        return Err(Error::invalid_padding());
                    }
                }
                break;
            }
            let decoded = table[ch as usize];
            if decoded < 0 {
                return Err(Error::invalid_byte(ch, chunk_index * 4 + i));
            }
            values[i] = decoded as u8;
        }

        if padding > 2 {
            return Err(Error::invalid_padding());
        }
        if padding > 0 && chunk_index + 1 != total_chunks {
            return Err(Error::invalid_padding());
        }

        let triple = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | (values[3] as u32);

        output.push(((triple >> 16) & 0xff) as u8);
        if effective_len > 2 {
            output.push(((triple >> 8) & 0xff) as u8);
        }
        if effective_len > 3 {
            output.push((triple & 0xff) as u8);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_standard() {
        let inputs = [
            b"".as_ref(),
            b"f",
            b"fo",
            b"foo",
            b"foobar",
            b"The quick brown fox jumps over the lazy dog",
        ];
        for data in inputs {
            let encoded = encode_standard(data);
            let decoded = decode_standard(&encoded).unwrap();
            assert_eq!(data, decoded.as_slice());
        }
    }

    #[test]
    fn roundtrip_url_no_pad() {
        let inputs: &[&[u8]] = &[
            b"",
            b"hello",
            b"base64",
            b"The quick brown fox",
            b"1234567890",
        ];
        for &data in inputs {
            let encoded = encode_url_no_pad(data);
            let decoded = decode_url_no_pad(&encoded).unwrap();
            assert_eq!(data, decoded.as_slice());
        }
    }

    #[test]
    fn decode_rejects_invalid_char() {
        let err = decode_standard("Zm9v?").unwrap_err();
        assert!(err.is_invalid_length() || err.is_invalid_byte());
    }

    #[test]
    fn decode_rejects_bad_padding() {
        assert!(decode_standard("Zm=0").unwrap_err().is_invalid_padding());
        assert!(decode_url_no_pad("Zm=0").unwrap_err().is_invalid_padding());
    }

    #[test]
    fn decode_rejects_length() {
        assert!(decode_standard("Z").unwrap_err().is_invalid_length());
        assert!(decode_url_no_pad("Z").unwrap_err().is_invalid_length());
    }
}
