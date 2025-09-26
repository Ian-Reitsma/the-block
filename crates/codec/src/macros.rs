//! Helper macros for integrating serde-friendly types with the codec API.

/// Implement [`crate::CodecMessage`] for a type that can be losslessly
/// converted to and from another serde-enabled type using `Into`/`From`.
#[macro_export]
macro_rules! codec_via_from_into {
    ($ty:ty, $proxy:ty) => {
        impl $crate::CodecMessage for $ty {
            fn encode_with(&self, codec: $crate::Codec) -> $crate::Result<Vec<u8>> {
                let proxy: $proxy = self.clone().into();
                $crate::serialize(codec, &proxy)
            }

            fn decode_with(bytes: &[u8], codec: $crate::Codec) -> $crate::Result<Self> {
                let proxy: $proxy = $crate::deserialize(codec, bytes)?;
                Ok(proxy.into())
            }
        }
    };
}

/// Implement [`crate::CodecMessage`] for a type using explicit conversion
/// functions.
#[macro_export]
macro_rules! codec_bridge {
    ($ty:ty, $proxy:ty, $into:expr, $from:expr) => {
        impl $crate::CodecMessage for $ty {
            fn encode_with(&self, codec: $crate::Codec) -> $crate::Result<Vec<u8>> {
                let proxy: $proxy = ($into)(self);
                $crate::serialize(codec, &proxy)
            }

            fn decode_with(bytes: &[u8], codec: $crate::Codec) -> $crate::Result<Self> {
                let proxy: $proxy = $crate::deserialize(codec, bytes)?;
                Ok(($from)(proxy))
            }
        }
    };
}
