#![forbid(unsafe_code)]

use p2p_overlay::InhousePeerId;
use std::str;

/// Drive the peer ID decoder with arbitrary input.
pub fn run(data: &[u8]) {
    if let Ok(text) = str::from_utf8(data) {
        if let Ok(id) = InhousePeerId::from_base58(text) {
            let encoded = id.to_base58();
            let decoded = InhousePeerId::from_base58(&encoded).expect("re-encode should decode");
            assert_eq!(id.as_bytes(), decoded.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_utf8_sequences() {
        let bytes = [0xff, 0xfe, 0xfd];
        run(&bytes);
    }

    #[test]
    fn roundtrips_valid_id() {
        let id = InhousePeerId::new([0x11; 32]);
        let encoded = id.to_base58();
        run(encoded.as_bytes());
    }
}
