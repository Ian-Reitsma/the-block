use crypto_suite::hashing::blake3;

/// Derive a short correlation ID from a block height.
pub fn corr_id_height(height: u64) -> String {
    format!("{:016x}", height)
}

/// Derive a short correlation ID from a hash value.
pub fn corr_id_hash(hash: &[u8]) -> String {
    let h = blake3::hash(hash);
    crypto_suite::hex::encode(&h.as_bytes()[0..8])
}

/// Generate a random correlation identifier for ad-hoc requests.
pub fn corr_id_random() -> String {
    use rand::{rngs::OsRng, RngCore};
    let mut bytes = [0u8; 8];
    OsRng::default().fill_bytes(&mut bytes);
    crypto_suite::hex::encode(bytes)
}

#[cfg_attr(not(feature = "telemetry"), allow(dead_code))]
pub(crate) fn info_span_with_field(
    name: &'static str,
    key: &'static str,
    value: String,
) -> diagnostics::tracing::Span {
    diagnostics::tracing::Span::new(
        std::borrow::Cow::Borrowed(name),
        diagnostics::tracing::Level::INFO,
        vec![diagnostics::FieldValue {
            key: std::borrow::Cow::Borrowed(key),
            value,
        }],
    )
}

#[macro_export]
macro_rules! log_context {
    (block = $height:expr) => {
        $crate::logging::info_span_with_field(
            "block",
            "correlation_id",
            $crate::logging::corr_id_height($height),
        )
    };
    (tx = $hash:expr) => {
        $crate::logging::info_span_with_field(
            "tx",
            "correlation_id",
            $crate::logging::corr_id_hash(&$hash),
        )
    };
    (request) => {
        $crate::logging::info_span_with_field(
            "request",
            "correlation_id",
            $crate::logging::corr_id_random(),
        )
    };
    (request = $id:expr) => {
        $crate::logging::info_span_with_field("request", "correlation_id", format!("{}", &$id))
    };
    (provider = $id:expr) => {
        $crate::logging::info_span_with_field("provider", "id", format!("{}", &$id))
    };
}
