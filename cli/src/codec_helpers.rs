use codec::{self, profiles};
use serde::{de::DeserializeOwned, Serialize};

pub fn json_from_str<T>(input: &str) -> codec::Result<T>
where
    T: DeserializeOwned,
{
    codec::deserialize_from_str(profiles::json(), input)
}

#[cfg(feature = "wasm-metadata")]
pub fn json_to_vec<T>(value: &T) -> codec::Result<Vec<u8>>
where
    T: Serialize,
{
    codec::serialize(profiles::json(), value)
}

pub fn json_to_string<T>(value: &T) -> codec::Result<String>
where
    T: Serialize,
{
    codec::serialize_to_string(profiles::json(), value)
}

pub fn json_to_string_pretty<T>(value: &T) -> codec::Result<String>
where
    T: Serialize,
{
    codec::serialize_json_pretty(value)
}
