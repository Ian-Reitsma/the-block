use core::fmt;

/// Error returned when decoding hex strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input contains a non-hexadecimal character.
    InvalidDigit { index: usize, byte: u8 },
    /// The input does not contain an even number of digits.
    OddLength,
    /// The decoded output does not match the requested length.
    LengthMismatch { expected: usize, actual: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidDigit { index, byte } => {
                write!(f, "invalid hex digit at index {index}: 0x{byte:02x}")
            }
            Error::OddLength => write!(f, "hex input must contain an even number of characters"),
            Error::LengthMismatch { expected, actual } => write!(
                f,
                "decoded hex length mismatch: expected {expected} bytes, got {actual}"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Encode the provided bytes as a lowercase hex string.
pub fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Decode the provided hex string into a byte vector.
pub fn decode(input: &str) -> Result<Vec<u8>, Error> {
    if input.len() % 2 != 0 {
        return Err(Error::OddLength);
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut chars = input.as_bytes().iter().enumerate();
    while let Some((hi_idx, &hi)) = chars.next() {
        let (lo_idx, &lo) = match chars.next() {
            Some(pair) => pair,
            None => return Err(Error::OddLength),
        };
        let high = decode_nibble(hi).ok_or(Error::InvalidDigit {
            index: hi_idx,
            byte: hi,
        })?;
        let low_idx = lo_idx;
        let low = decode_nibble(lo).ok_or(Error::InvalidDigit {
            index: low_idx,
            byte: lo,
        })?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

/// Decode the provided hex string into a fixed-size array.
pub fn decode_array<const N: usize>(input: &str) -> Result<[u8; N], Error> {
    let bytes = decode(input)?;
    if bytes.len() != N {
        return Err(Error::LengthMismatch {
            expected: N,
            actual: bytes.len(),
        });
    }
    let mut array = [0u8; N];
    array.copy_from_slice(&bytes);
    Ok(array)
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_roundtrip() {
        let data = [0x00u8, 0x12, 0xab, 0xff];
        let encoded = encode(&data);
        assert_eq!(encoded, "0012abff");
        let decoded = decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn decode_array_length_mismatch() {
        let err = decode_array::<4>("00").unwrap_err();
        assert!(matches!(
            err,
            Error::LengthMismatch {
                expected: 4,
                actual: 1
            }
        ));
    }

    #[test]
    fn decode_invalid_digit() {
        let err = decode("00xz").unwrap_err();
        assert!(matches!(err, Error::InvalidDigit { index: 2, .. }));
    }
}
