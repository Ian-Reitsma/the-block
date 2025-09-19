#![cfg(feature = "integration-tests")]
use the_block::util::versioned_blob::{
    decode_blob, encode_blob, DecodeErr, Versioned, MAGIC_PRICE_BOARD,
};

#[derive(Debug, PartialEq)]
struct Example(Vec<u8>);

impl Versioned for Example {
    const VERSION: u16 = 1;
    fn encode(&self) -> Vec<u8> {
        self.0.clone()
    }
    fn decode_v(version: u16, bytes: &[u8]) -> Result<Self, DecodeErr> {
        if version != Self::VERSION {
            return Err(DecodeErr::UnsupportedVersion { found: version });
        }
        Ok(Example(bytes.to_vec()))
    }
}

#[test]
fn bad_magic() {
    let payload = b"hello";
    let blob = encode_blob(MAGIC_PRICE_BOARD, Example::VERSION, payload);
    assert_eq!(
        decode_blob(&blob, *b"BADC").unwrap_err(),
        DecodeErr::BadMagic
    );
}

#[test]
fn version_mismatch() {
    let payload = Example(b"abc".to_vec());
    let blob = encode_blob(MAGIC_PRICE_BOARD, Example::VERSION, &payload.encode());
    let (_v, bytes) = decode_blob(&blob, MAGIC_PRICE_BOARD).unwrap();
    assert_eq!(
        Example::decode_v(Example::VERSION + 1, bytes).unwrap_err(),
        DecodeErr::UnsupportedVersion {
            found: Example::VERSION + 1
        }
    );
}

#[test]
fn bad_crc() {
    let payload = b"world";
    let mut blob = encode_blob(MAGIC_PRICE_BOARD, Example::VERSION, payload);
    let last = blob.last_mut().unwrap();
    *last ^= 0xFF;
    assert_eq!(
        decode_blob(&blob, MAGIC_PRICE_BOARD).unwrap_err(),
        DecodeErr::BadCrc
    );
}
