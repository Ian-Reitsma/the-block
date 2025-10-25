use core::fmt;

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidByte { byte: u8, index: usize },
}

impl Error {
    fn invalid(byte: u8, index: usize) -> Self {
        Error::InvalidByte { byte, index }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidByte { byte, index } => {
                if (32..=126).contains(byte) || *byte == b'\t' || *byte == b'\n' || *byte == b'\r' {
                    write!(
                        f,
                        "invalid base58 character '{}' at index {index}",
                        *byte as char
                    )
                } else {
                    write!(f, "invalid base58 byte 0x{byte:02x} at index {index}")
                }
            }
        }
    }
}

impl std::error::Error for Error {}

fn value_for(byte: u8) -> Option<u8> {
    match byte {
        b'1' => Some(0),
        b'2' => Some(1),
        b'3' => Some(2),
        b'4' => Some(3),
        b'5' => Some(4),
        b'6' => Some(5),
        b'7' => Some(6),
        b'8' => Some(7),
        b'9' => Some(8),
        b'A' => Some(9),
        b'B' => Some(10),
        b'C' => Some(11),
        b'D' => Some(12),
        b'E' => Some(13),
        b'F' => Some(14),
        b'G' => Some(15),
        b'H' => Some(16),
        b'J' => Some(17),
        b'K' => Some(18),
        b'L' => Some(19),
        b'M' => Some(20),
        b'N' => Some(21),
        b'P' => Some(22),
        b'Q' => Some(23),
        b'R' => Some(24),
        b'S' => Some(25),
        b'T' => Some(26),
        b'U' => Some(27),
        b'V' => Some(28),
        b'W' => Some(29),
        b'X' => Some(30),
        b'Y' => Some(31),
        b'Z' => Some(32),
        b'a' => Some(33),
        b'b' => Some(34),
        b'c' => Some(35),
        b'd' => Some(36),
        b'e' => Some(37),
        b'f' => Some(38),
        b'g' => Some(39),
        b'h' => Some(40),
        b'i' => Some(41),
        b'j' => Some(42),
        b'k' => Some(43),
        b'm' => Some(44),
        b'n' => Some(45),
        b'o' => Some(46),
        b'p' => Some(47),
        b'q' => Some(48),
        b'r' => Some(49),
        b's' => Some(50),
        b't' => Some(51),
        b'u' => Some(52),
        b'v' => Some(53),
        b'w' => Some(54),
        b'x' => Some(55),
        b'y' => Some(56),
        b'z' => Some(57),
        _ => None,
    }
}

pub fn encode(input: &[u8]) -> String {
    if input.is_empty() {
        return String::new();
    }

    let mut zeros = 0usize;
    while zeros < input.len() && input[zeros] == 0 {
        zeros += 1;
    }

    let mut digits: Vec<u8> = Vec::new();
    for &byte in &input[zeros..] {
        let mut carry = byte as u32;
        for digit in digits.iter_mut() {
            let acc = (*digit as u32) * 256 + carry;
            *digit = (acc % 58) as u8;
            carry = acc / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }

    let mut encoded = String::with_capacity(zeros + digits.len());
    for _ in 0..zeros {
        encoded.push('1');
    }
    for digit in digits.iter().rev() {
        encoded.push(ALPHABET[*digit as usize] as char);
    }

    if digits.is_empty() {
        // The value consisted solely of leading zeros, ensure at least one character.
        // The loop above already emitted the correct count of '1's, so nothing else to do.
    }

    encoded
}

pub fn decode(input: &str) -> Result<Vec<u8>, Error> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut zeros = 0usize;
    let bytes = input.as_bytes();
    while zeros < bytes.len() && bytes[zeros] == b'1' {
        zeros += 1;
    }

    let mut decoded: Vec<u8> = Vec::new();
    for (index, &byte) in bytes.iter().enumerate() {
        let value = match value_for(byte) {
            Some(v) => v as u32,
            None => return Err(Error::invalid(byte, index)),
        };
        let mut carry = value;
        for digit in decoded.iter_mut() {
            let acc = (*digit as u32) * 58 + carry;
            *digit = (acc & 0xff) as u8;
            carry = acc >> 8;
        }
        while carry > 0 {
            decoded.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }

    decoded.extend(std::iter::repeat_n(0, zeros));

    decoded.reverse();

    // Remove any leading zeros that were introduced by the conversion loop but
    // were not present in the original representation (except those required by
    // the encoded zero-prefix).
    let mut first_non_zero = 0;
    while first_non_zero < decoded.len() && decoded[first_non_zero] == 0 {
        first_non_zero += 1;
    }
    if first_non_zero > zeros {
        decoded.drain(zeros..first_non_zero);
    }

    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_basic_values() {
        let samples = [
            b"".as_ref(),
            b"\x00".as_ref(),
            b"\x00\x00".as_ref(),
            b"\x01".as_ref(),
            b"hello".as_ref(),
            b"The quick brown fox jumps over the lazy dog".as_ref(),
        ];
        for sample in samples {
            let encoded = encode(sample);
            let decoded = decode(&encoded).expect("decode");
            assert_eq!(decoded, sample, "round-trip for {sample:?}");
        }
    }

    #[test]
    fn known_vectors() {
        let cases = [
            ("", b"".as_ref()),
            ("1", b"\x00".as_ref()),
            ("11", b"\x00\x00".as_ref()),
            ("JxF12TrwUP45BMd", b"Hello World".as_ref()),
        ];
        for (encoded, raw) in cases {
            assert_eq!(decode(encoded).expect("decode"), raw);
            assert_eq!(encode(raw), encoded);
        }
    }

    #[test]
    fn invalid_character_rejected() {
        let err = decode("0OIl").unwrap_err();
        match err {
            Error::InvalidByte { .. } => {}
        }
    }
}
