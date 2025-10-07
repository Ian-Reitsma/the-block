use core::fmt;
use crypto_suite::hashing::crc32;
use crypto_suite::{Error as CryptoError, ErrorKind as CryptoErrorKind};

pub const MAGIC_PRICE_BOARD: [u8; 4] = *b"PBRD";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeErr {
    Unimplemented(&'static str),
}

impl EncodeErr {
    const fn unimplemented(component: &'static str) -> Self {
        Self::Unimplemented(component)
    }
}

impl fmt::Display for EncodeErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeErr::Unimplemented(component) => {
                write!(f, "{component} is not yet implemented")
            }
        }
    }
}

impl std::error::Error for EncodeErr {}

impl From<CryptoError> for EncodeErr {
    fn from(err: CryptoError) -> Self {
        match err.kind() {
            CryptoErrorKind::Unimplemented(component) => Self::unimplemented(component),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeErr {
    BadMagic,
    BadCrc,
    UnsupportedVersion { found: u16 },
    Unimplemented(&'static str),
}

impl DecodeErr {
    const fn unimplemented(component: &'static str) -> Self {
        Self::Unimplemented(component)
    }
}

impl fmt::Display for DecodeErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeErr::BadMagic => write!(f, "bad magic"),
            DecodeErr::BadCrc => write!(f, "bad crc"),
            DecodeErr::UnsupportedVersion { found } => {
                write!(f, "unsupported version {found}")
            }
            DecodeErr::Unimplemented(component) => {
                write!(f, "{component} is not yet implemented")
            }
        }
    }
}

impl std::error::Error for DecodeErr {}

impl From<CryptoError> for DecodeErr {
    fn from(err: CryptoError) -> Self {
        match err.kind() {
            CryptoErrorKind::Unimplemented(component) => Self::unimplemented(component),
        }
    }
}

pub fn encode_blob(magic: [u8; 4], version: u16, payload: &[u8]) -> Result<Vec<u8>, EncodeErr> {
    let mut buf = Vec::with_capacity(4 + 2 + 4 + payload.len() + 4);
    buf.extend_from_slice(&magic);
    buf.extend_from_slice(&version.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);

    let crc = crc32::checksum(&buf)?;
    buf.extend_from_slice(&crc.to_le_bytes());
    Ok(buf)
}

pub fn decode_blob<'a>(
    bytes: &'a [u8],
    expect_magic: [u8; 4],
) -> Result<(u16, &'a [u8]), DecodeErr> {
    if bytes.len() < 4 + 2 + 4 + 4 {
        return Err(DecodeErr::BadCrc);
    }
    let magic = &bytes[0..4];
    if magic != expect_magic {
        return Err(DecodeErr::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let len = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
    let payload_start = 10;
    let payload_end = payload_start + len;
    if bytes.len() != payload_end + 4 {
        return Err(DecodeErr::BadCrc);
    }
    let payload = &bytes[payload_start..payload_end];
    let stored_crc = u32::from_le_bytes([
        bytes[payload_end],
        bytes[payload_end + 1],
        bytes[payload_end + 2],
        bytes[payload_end + 3],
    ]);
    let crc = crc32::checksum(&bytes[..payload_end]).map_err(DecodeErr::from)?;
    if crc != stored_crc {
        return Err(DecodeErr::BadCrc);
    }
    Ok((version, payload))
}

pub trait Versioned {
    const VERSION: u16;
    fn encode(&self) -> Result<Vec<u8>, EncodeErr>;
    fn decode_v(version: u16, bytes: &[u8]) -> Result<Self, DecodeErr>
    where
        Self: Sized;
}
