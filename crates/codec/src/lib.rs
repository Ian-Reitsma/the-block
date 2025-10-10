use core::fmt;

use foundation_serialization::{binary, json, Error as SerializationError};
use serde::{de::DeserializeOwned, Serialize};
use std::string::FromUtf8Error;
#[cfg(feature = "telemetry")]
use std::sync::OnceLock;
use thiserror::Error;

#[cfg(feature = "telemetry")]
use metrics::{histogram, increment_counter};

pub mod inhouse;
pub mod macros;
pub mod profiles;

/// Semantic version of the codec crate exposed for telemetry labeling.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "telemetry")]
type MetricsHook = fn(Codec, Direction, Option<usize>);

#[cfg(feature = "telemetry")]
static METRICS_HOOK: OnceLock<MetricsHook> = OnceLock::new();

#[cfg(feature = "telemetry")]
#[derive(Debug, thiserror::Error)]
pub enum MetricsHookError {
    #[error("codec telemetry hook already installed")]
    AlreadyInstalled,
}

#[cfg(feature = "telemetry")]
pub fn install_metrics_hook(hook: MetricsHook) -> std::result::Result<(), MetricsHookError> {
    METRICS_HOOK
        .set(hook)
        .map_err(|_| MetricsHookError::AlreadyInstalled)
}

/// Result alias using the codec error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Direction of a codec operation.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    /// Serializing a structure into bytes.
    Serialize,
    /// Deserializing bytes into a structure.
    Deserialize,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Direction::Serialize => write!(f, "serialize"),
            Direction::Deserialize => write!(f, "deserialize"),
        }
    }
}

impl Direction {
    #[cfg(feature = "telemetry")]
    const fn as_str(self) -> &'static str {
        match self {
            Direction::Serialize => "serialize",
            Direction::Deserialize => "deserialize",
        }
    }
}

/// Binary profiles made available to the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryProfile {
    /// Canonical first-party binary serializer used for general payloads.
    Canonical,
    /// Transaction serialization profile.
    Transaction,
    /// Gossip relay persistence profile.
    Gossip,
    /// Storage manifest persistence profile.
    StorageManifest,
}

impl BinaryProfile {
    const fn as_str(self) -> &'static str {
        match self {
            BinaryProfile::Canonical => "canonical",
            BinaryProfile::Transaction => "transaction",
            BinaryProfile::Gossip => "gossip",
            BinaryProfile::StorageManifest => "storage_manifest",
        }
    }

    fn encode<T: Serialize>(self, value: &T) -> std::result::Result<Vec<u8>, SerializationError> {
        binary::encode(value)
    }

    fn decode<T: DeserializeOwned>(
        self,
        bytes: &[u8],
    ) -> std::result::Result<T, SerializationError> {
        binary::decode(bytes)
    }

    /// Fetch the configured profile wrapper for this codec.
    #[must_use]
    pub const fn config(self) -> BinaryConfig {
        BinaryConfig { profile: self }
    }
}

impl fmt::Display for BinaryProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Wrapper retaining the public API exposed by `codec` for binary profiles.
#[derive(Clone, Copy, Debug)]
pub struct BinaryConfig {
    profile: BinaryProfile,
}

impl BinaryConfig {
    /// Serialize a value using the associated profile.
    pub fn serialize<T: Serialize>(
        &self,
        value: &T,
    ) -> std::result::Result<Vec<u8>, SerializationError> {
        self.profile.encode(value)
    }

    /// Deserialize a value using the associated profile.
    pub fn deserialize<T: DeserializeOwned>(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<T, SerializationError> {
        self.profile.decode(bytes)
    }

    /// Return the underlying profile.
    pub const fn profile(self) -> BinaryProfile {
        self.profile
    }
}

/// Canonical JSON configuration identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonProfile {
    /// Default JSON serializer from the first-party facade.
    Canonical,
}

impl fmt::Display for JsonProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonProfile::Canonical => write!(f, "canonical"),
        }
    }
}

/// Selects the codec implementation to use for serialization or deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// Binary serialization using one of the configured profiles.
    Binary(BinaryProfile),
    /// JSON serialization using the canonical settings.
    Json(JsonProfile),
}

impl Codec {
    fn codec_label(self) -> &'static str {
        match self {
            Codec::Binary(_) => "binary",
            Codec::Json(_) => "json",
        }
    }

    #[cfg(feature = "telemetry")]
    fn profile_label(self) -> Option<&'static str> {
        match self {
            Codec::Binary(profile) => Some(profile.as_str()),
            Codec::Json(_) => None,
        }
    }

    fn encode<T: Serialize>(self, value: &T) -> Result<Vec<u8>> {
        match self {
            Codec::Binary(profile) => profile
                .config()
                .serialize(value)
                .map_err(|err| Error::from_binary(err, profile, Direction::Serialize)),
            Codec::Json(JsonProfile::Canonical) => {
                json::to_vec(value).map_err(|err| Error::from_json(err, Direction::Serialize))
            }
        }
    }

    fn decode<T: DeserializeOwned>(self, bytes: &[u8]) -> Result<T> {
        match self {
            Codec::Binary(profile) => profile
                .config()
                .deserialize(bytes)
                .map_err(|err| Error::from_binary(err, profile, Direction::Deserialize)),
            Codec::Json(JsonProfile::Canonical) => {
                json::from_slice(bytes).map_err(|err| Error::from_json(err, Direction::Deserialize))
            }
        }
    }
}

/// Error raised when a codec operation fails.
#[derive(Debug, Error)]
pub enum Error {
    /// A binary codec failure using the first-party facade.
    #[error("{direction} using {profile} binary profile failed: {source}")]
    Binary {
        /// Underlying error reported by the serialization facade.
        #[source]
        source: SerializationError,
        /// Named profile describing the configuration that failed.
        profile: BinaryProfile,
        /// Direction of the codec operation.
        direction: Direction,
    },
    /// A JSON codec failure using the first-party facade.
    #[error("{direction} using JSON codec failed: {source}")]
    Json {
        /// Underlying JSON error reported by the serialization facade.
        #[source]
        source: SerializationError,
        /// Direction of the codec operation.
        direction: Direction,
    },
    /// UTF-8 conversion failure when emitting textual payloads.
    #[error("{direction} using {codec} codec produced invalid UTF-8: {source}")]
    Utf8 {
        /// Underlying UTF-8 conversion error.
        #[source]
        source: FromUtf8Error,
        /// Codec responsible for the failure.
        codec: &'static str,
        /// Direction of the codec operation.
        direction: Direction,
    },
    /// Attempted to use an unsupported text conversion for the codec.
    #[error("{direction} as string is unsupported for the {codec} codec")]
    UnsupportedTextCodec {
        /// Codec identifier that does not support text conversion.
        codec: &'static str,
        /// Direction of the codec operation.
        direction: Direction,
    },
}

impl Error {
    fn from_binary(
        source: SerializationError,
        profile: BinaryProfile,
        direction: Direction,
    ) -> Self {
        Error::Binary {
            source,
            profile,
            direction,
        }
    }

    fn from_json(source: SerializationError, direction: Direction) -> Self {
        Error::Json { source, direction }
    }

    fn from_utf8(source: FromUtf8Error, codec: &'static str, direction: Direction) -> Self {
        Error::Utf8 {
            source,
            codec,
            direction,
        }
    }

    fn unsupported_text(codec: &'static str, direction: Direction) -> Self {
        Error::UnsupportedTextCodec { codec, direction }
    }
}

/// Serialize `value` using the provided `codec` configuration.
#[must_use]
pub fn serialize<T: Serialize>(codec: Codec, value: &T) -> Result<Vec<u8>> {
    let result = codec.encode(value);
    observe_result(
        result.as_ref().ok().map(|bytes| bytes.len()),
        codec,
        Direction::Serialize,
    );
    result
}

/// Deserialize `bytes` into `T` using the provided `codec` configuration.
pub fn deserialize<T>(codec: Codec, bytes: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    let result = codec.decode(bytes);
    observe_result(
        result.as_ref().ok().map(|_| bytes.len()),
        codec,
        Direction::Deserialize,
    );
    result
}

/// Deserialize a UTF-8 string into `T` using the provided `codec` configuration.
pub fn deserialize_from_str<T>(codec: Codec, value: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    deserialize(codec, value.as_bytes())
}

/// Serialize `value` using the provided `codec` and return a UTF-8 string when supported.
pub fn serialize_to_string<T: Serialize>(codec: Codec, value: &T) -> Result<String> {
    match codec {
        Codec::Json(_) => {
            serialize(codec, value).and_then(|bytes| match String::from_utf8(bytes) {
                Ok(string) => Ok(string),
                Err(err) => {
                    observe_result(None, codec, Direction::Serialize);
                    Err(Error::from_utf8(
                        err,
                        codec.codec_label(),
                        Direction::Serialize,
                    ))
                }
            })
        }
        Codec::Binary(_) => Err(Error::unsupported_text(
            codec.codec_label(),
            Direction::Serialize,
        )),
    }
}

/// Serialize `value` to a prettified JSON string using the canonical settings.
pub fn serialize_json_pretty<T: Serialize>(value: &T) -> Result<String> {
    let result =
        json::to_string_pretty(value).map_err(|err| Error::from_json(err, Direction::Serialize));
    observe_result(
        result.as_ref().ok().map(|s| s.as_bytes().len()),
        Codec::Json(JsonProfile::Canonical),
        Direction::Serialize,
    );
    result
}

/// Trait bridging serde-enabled types into codec-aware helpers.
pub trait CodecMessage: Sized {
    /// Serialize the message with the supplied codec profile.
    fn encode_with(&self, codec: Codec) -> Result<Vec<u8>>;
    /// Deserialize the message from the supplied codec profile.
    fn decode_with(bytes: &[u8], codec: Codec) -> Result<Self>;
}

impl<T> CodecMessage for T
where
    T: Serialize + DeserializeOwned,
{
    fn encode_with(&self, codec: Codec) -> Result<Vec<u8>> {
        serialize(codec, self)
    }

    fn decode_with(bytes: &[u8], codec: Codec) -> Result<Self> {
        deserialize(codec, bytes)
    }
}

#[cfg(feature = "telemetry")]
fn observe_result(size: Option<usize>, codec: Codec, direction: Direction) {
    if let Some(hook) = METRICS_HOOK.get() {
        hook(codec, direction, size);
    }
    match size {
        Some(len) => record_success(codec, direction, len),
        _ => record_failure(codec, direction),
    }
}

#[cfg(not(feature = "telemetry"))]
fn observe_result(_size: Option<usize>, _codec: Codec, _direction: Direction) {}

#[cfg(feature = "telemetry")]
fn record_success(codec: Codec, direction: Direction, size: usize) {
    let codec_label = codec.codec_label();
    let direction_label = direction.as_str();
    if let Some(profile) = codec.profile_label() {
        histogram!(
            "codec_payload_bytes",
            size as f64,
            "codec" => codec_label,
            "direction" => direction_label,
            "profile" => profile,
        );
    } else {
        histogram!(
            "codec_payload_bytes",
            size as f64,
            "codec" => codec_label,
            "direction" => direction_label,
        );
    }
}

#[cfg(feature = "telemetry")]
fn record_failure(codec: Codec, direction: Direction) {
    let codec_label = codec.codec_label();
    let direction_label = direction.as_str();
    if let Some(profile) = codec.profile_label() {
        increment_counter!(
            "codec_operation_fail_total",
            "codec" => codec_label,
            "direction" => direction_label,
            "profile" => profile,
        );
    } else {
        increment_counter!(
            "codec_operation_fail_total",
            "codec" => codec_label,
            "direction" => direction_label,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Sample {
        value: u32,
    }

    #[test]
    fn binary_roundtrip_uses_named_profile() {
        let sample = Sample { value: 42 };
        let bytes = serialize(Codec::Binary(BinaryProfile::Transaction), &sample).unwrap();
        let via_config = profiles::transaction::config()
            .serialize(&sample)
            .expect("transaction profile");
        assert_eq!(bytes, via_config);
        let decoded: Sample =
            deserialize(Codec::Binary(BinaryProfile::Transaction), &bytes).unwrap();
        assert_eq!(decoded, sample);
    }

    #[test]
    fn json_roundtrip() {
        let sample = Sample { value: 7 };
        let bytes = serialize(Codec::Json(JsonProfile::Canonical), &sample).unwrap();
        let decoded: Sample = deserialize(Codec::Json(JsonProfile::Canonical), &bytes).unwrap();
        assert_eq!(decoded, sample);
    }

    #[test]
    fn binary_roundtrip() {
        let sample = Sample { value: 9 };
        let bytes = serialize(Codec::Binary(BinaryProfile::Canonical), &sample).unwrap();
        let decoded: Sample = deserialize(Codec::Binary(BinaryProfile::Canonical), &bytes).unwrap();
        assert_eq!(decoded, sample);
    }

    #[test]
    fn json_string_roundtrip_helpers() {
        let sample = Sample { value: 11 };
        let text = serialize_to_string(Codec::Json(JsonProfile::Canonical), &sample).unwrap();
        let decoded: Sample =
            deserialize_from_str(Codec::Json(JsonProfile::Canonical), &text).unwrap();
        assert_eq!(decoded, sample);

        let pretty = serialize_json_pretty(&sample).unwrap();
        assert!(pretty.contains("value"));
    }

    #[test]
    fn corrupted_payloads_return_errors() {
        let sample = Sample { value: 13 };
        let mut bytes = serialize(Codec::Binary(BinaryProfile::Transaction), &sample).unwrap();
        bytes.pop();
        let err =
            deserialize::<Sample>(Codec::Binary(BinaryProfile::Transaction), &bytes).unwrap_err();
        assert!(matches!(err, Error::Binary { .. }));

        let json_err =
            deserialize::<Sample>(Codec::Json(JsonProfile::Canonical), b"not json").unwrap_err();
        assert!(matches!(json_err, Error::Json { .. }));

        let binary_err =
            deserialize::<Sample>(Codec::Binary(BinaryProfile::Canonical), b"not binary")
                .unwrap_err();
        assert!(matches!(binary_err, Error::Binary { .. }));
    }

    #[test]
    fn unsupported_text_codec_errors() {
        let sample = Sample { value: 21 };
        let err =
            serialize_to_string(Codec::Binary(BinaryProfile::Transaction), &sample).unwrap_err();
        assert!(matches!(err, Error::UnsupportedTextCodec { .. }));
    }
}
