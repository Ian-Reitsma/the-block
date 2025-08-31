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
