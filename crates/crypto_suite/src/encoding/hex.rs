use core::fmt;

/// Errors that can occur when encoding or decoding hexadecimal data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The provided input had an odd number of hexadecimal digits.
    OddLength { len: usize },
    /// The provided buffer length does not match the expected size.
    InvalidLength { expected: usize, actual: usize },
    /// A character outside the hexadecimal alphabet was encountered.
    InvalidChar { index: usize, byte: u8 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OddLength { len } => write!(f, "hex string has odd length: {len}"),
            Error::InvalidLength { expected, actual } => {
                write!(
                    f,
                    "hex buffer length mismatch: expected {expected}, got {actual}"
                )
            }
            Error::InvalidChar { index, byte } => {
                write!(f, "invalid hex character at index {index}: 0x{byte:02x}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Encode arbitrary bytes into a lower-case hexadecimal string.
pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
    let bytes = input.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_TABLE[(byte >> 4) as usize]);
        out.push(HEX_TABLE[(byte & 0x0f) as usize]);
    }
    out
}

/// Encode into the provided byte slice as ASCII lower-case hex digits.
pub fn encode_to_slice<T: AsRef<[u8]>>(input: T, out: &mut [u8]) -> Result<(), Error> {
    let bytes = input.as_ref();
    let expected = bytes.len() * 2;
    if out.len() != expected {
        return Err(Error::InvalidLength {
            expected,
            actual: out.len(),
        });
    }

    for (i, &byte) in bytes.iter().enumerate() {
        out[i * 2] = HEX_TABLE[(byte >> 4) as usize] as u8;
        out[i * 2 + 1] = HEX_TABLE[(byte & 0x0f) as usize] as u8;
    }
    Ok(())
}

/// Decode a hexadecimal string into a Vec of bytes.
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, Error> {
    let input = input.as_ref();
    let mut out = vec![0u8; decode_len(input)?];
    decode_to_slice_internal(input, &mut out)?;
    Ok(out)
}

/// Decode into a fixed slice. The slice length must be exactly half the input length.
pub fn decode_to_slice<T: AsRef<[u8]>>(input: T, out: &mut [u8]) -> Result<(), Error> {
    let input = input.as_ref();
    let expected = decode_len(input)?;
    if out.len() != expected {
        return Err(Error::InvalidLength {
            expected,
            actual: out.len(),
        });
    }
    decode_to_slice_internal(input, out)
}

/// Decode into a fixed-size array.
pub fn decode_array<const N: usize>(input: &str) -> Result<[u8; N], Error> {
    let mut out = [0u8; N];
    decode_to_slice(input, &mut out)?;
    Ok(out)
}

fn decode_len(input: &[u8]) -> Result<usize, Error> {
    if input.len() % 2 != 0 {
        return Err(Error::OddLength { len: input.len() });
    }
    Ok(input.len() / 2)
}

fn decode_to_slice_internal(input: &[u8], out: &mut [u8]) -> Result<(), Error> {
    for (i, chunk) in input.chunks_exact(2).enumerate() {
        let high = decode_nibble(chunk[0]).ok_or(Error::InvalidChar {
            index: i * 2,
            byte: chunk[0],
        })?;
        let low = decode_nibble(chunk[1]).ok_or(Error::InvalidChar {
            index: i * 2 + 1,
            byte: chunk[1],
        })?;
        out[i] = (high << 4) | low;
    }
    Ok(())
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const HEX_TABLE: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_basic() {
        assert_eq!(encode([0u8, 1, 15, 16, 255]), "00010f10ff");
    }

    #[test]
    fn encode_to_slice_validates_length() {
        let mut buf = [0u8; 4];
        let err = encode_to_slice([0u8, 1], &mut buf[..3]).unwrap_err();
        assert!(matches!(err, Error::InvalidLength { .. }));
    }

    #[test]
    fn decode_basic() {
        assert_eq!(decode("deadbeef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn decode_array_exact() {
        let arr = decode_array::<4>("01234567").unwrap();
        assert_eq!(arr, [0x01, 0x23, 0x45, 0x67]);
    }

    #[test]
    fn decode_rejects_invalid_char() {
        let err = decode("zz").unwrap_err();
        assert!(matches!(err, Error::InvalidChar { index: 0, .. }));
    }

    #[test]
    fn decode_rejects_odd_length() {
        let err = decode("abc").unwrap_err();
        assert!(matches!(err, Error::OddLength { .. }));
    }
}
