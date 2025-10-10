use codec::{self, BinaryProfile, Codec, Error as CodecError};
use foundation_serialization::{de::DeserializeOwned, Serialize};

/// Serialize a value using the canonical in-house binary profile.
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    codec::serialize(Codec::Binary(BinaryProfile::Canonical), value)
}

/// Serialize a value into the provided buffer using the canonical binary profile.
pub fn serialize_into<T: Serialize>(value: &T, buffer: &mut Vec<u8>) -> Result<(), CodecError> {
    buffer.clear();
    let encoded = serialize(value)?;
    buffer.extend_from_slice(&encoded);
    Ok(())
}

/// Deserialize a value from bytes produced by the canonical binary profile.
pub fn deserialize<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    codec::deserialize(Codec::Binary(BinaryProfile::Canonical), bytes)
}
