use crc32fast::Hasher;

pub const MAGIC_PRICE_BOARD: [u8; 4] = *b"PBRD";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeErr {
    #[error("bad magic")]
    BadMagic,
    #[error("bad crc")]
    BadCrc,
    #[error("unsupported version {found}")]
    UnsupportedVersion { found: u16 },
}

pub fn encode_blob(magic: [u8; 4], version: u16, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + 2 + 4 + payload.len() + 4);
    buf.extend_from_slice(&magic);
    buf.extend_from_slice(&version.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);

    let mut hasher = Hasher::new();
    hasher.update(&buf);
    let crc = hasher.finalize();
    buf.extend_from_slice(&crc.to_le_bytes());
    buf
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
    let mut hasher = Hasher::new();
    hasher.update(&bytes[..payload_end]);
    let crc = hasher.finalize();
    if crc != stored_crc {
        return Err(DecodeErr::BadCrc);
    }
    Ok((version, payload))
}

pub trait Versioned {
    const VERSION: u16;
    fn encode(&self) -> Vec<u8>;
    fn decode_v(version: u16, bytes: &[u8]) -> Result<Self, DecodeErr>
    where
        Self: Sized;
}
