/// Minimal ABI utilities. Real implementation should support full encoding rules.
#[must_use]
pub fn encode_u64(v: u64) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

#[must_use]
pub fn decode_u64(buf: &[u8]) -> Option<u64> {
    if buf.len() >= 8 {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&buf[..8]);
        Some(u64::from_le_bytes(arr))
    } else {
        None
    }
}

/// Encode gas limit and price as 16 bytes (little endian).
#[must_use]
pub fn encode_gas(limit: u64, price: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&limit.to_le_bytes());
    out.extend_from_slice(&price.to_le_bytes());
    out
}

/// Decode gas limit and price from 16 bytes.
#[must_use]
pub fn decode_gas(buf: &[u8]) -> Option<(u64, u64)> {
    if buf.len() >= 16 {
        let mut l = [0u8; 8];
        let mut p = [0u8; 8];
        l.copy_from_slice(&buf[..8]);
        p.copy_from_slice(&buf[8..16]);
        Some((u64::from_le_bytes(l), u64::from_le_bytes(p)))
    } else {
        None
    }
}
