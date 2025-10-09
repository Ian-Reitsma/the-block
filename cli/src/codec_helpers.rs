use codec::{self, profiles};
use foundation_serialization::{de::DeserializeOwned, Serialize};

pub fn json_from_str<T>(input: &str) -> codec::Result<T>
where
    T: DeserializeOwned,
{
    codec::deserialize_from_str(profiles::json::codec(), input)
}

#[cfg(feature = "wasm-metadata")]
pub fn json_to_vec<T>(value: &T) -> codec::Result<Vec<u8>>
where
    T: Serialize,
{
    codec::serialize(profiles::json::codec(), value)
}

pub fn json_to_string<T>(value: &T) -> codec::Result<String>
where
    T: Serialize,
{
    codec::serialize_to_string(profiles::json::codec(), value)
}

pub fn json_to_string_pretty<T>(value: &T) -> codec::Result<String>
where
    T: Serialize,
{
    codec::serialize_json_pretty(value)
}
