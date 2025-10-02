//! Minimal first-party codec primitives.
//!
//! This module provides deterministic binary and JSON encoders that are
//! purposely limited to a small subset of types.  The intent is to supply a
//! concrete target for migrating the workspace away from serde/bincode while
//! allowing call-sites to opt into the new traits incrementally.

mod binary;
mod json;

pub use binary::{BinaryDecode, BinaryEncoder, BinaryWriter};
pub use json::{JsonEncode, JsonEncoder, JsonWriter};

/// Shared error type for the in-house codec primitives.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("malformed utf-8 string")]
    InvalidUtf8,
    #[error("invalid boolean discriminant: {0}")]
    InvalidBool(u8),
    #[error("invalid length: {0}")]
    InvalidLength(usize),
    #[error("custom error: {0}")]
    Custom(&'static str),
}

/// Result alias for in-house codec operations.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct Sample {
        id: u32,
        name: String,
        flag: bool,
        scores: Vec<u8>,
    }

    impl BinaryEncoder for Sample {
        fn encode_binary(&self, writer: &mut BinaryWriter) {
            writer.write_u32(self.id);
            writer.write_string(&self.name);
            writer.write_bool(self.flag);
            writer.write_bytes(&self.scores);
        }
    }

    impl BinaryDecode for Sample {
        fn decode_binary(input: &mut &[u8]) -> Result<Self> {
            Ok(Self {
                id: BinaryWriter::read_u32(input)?,
                name: BinaryWriter::read_string(input)?,
                flag: BinaryWriter::read_bool(input)?,
                scores: BinaryWriter::read_bytes(input)?,
            })
        }
    }

    impl JsonEncode for Sample {
        fn encode_json(&self, writer: &mut JsonWriter) {
            writer.begin_object();
            writer.object_key("id");
            writer.number(self.id as u64);
            writer.object_key("name");
            writer.string(&self.name);
            writer.object_key("flag");
            writer.boolean(self.flag);
            writer.object_key("scores");
            writer.begin_array();
            for value in &self.scores {
                writer.number((*value).into());
            }
            writer.end_array();
            writer.end_object();
        }
    }

    #[test]
    fn binary_roundtrip() {
        let sample = Sample {
            id: 17,
            name: "example".to_owned(),
            flag: true,
            scores: vec![1, 2, 3],
        };
        let mut writer = BinaryWriter::default();
        sample.encode_binary(&mut writer);
        let storage = writer.finish();
        let mut view: &[u8] = &storage;
        let decoded = Sample::decode_binary(&mut view).expect("decode");
        assert_eq!(decoded, sample);
        assert!(view.is_empty());
    }

    #[test]
    fn json_render() {
        let sample = Sample {
            id: 9,
            name: "json".to_owned(),
            flag: false,
            scores: vec![2, 4],
        };
        let mut writer = JsonWriter::default();
        sample.encode_json(&mut writer);
        assert_eq!(
            writer.finish(),
            "{\"id\":9,\"name\":\"json\",\"flag\":false,\"scores\":[2,4]}"
        );
    }
}
