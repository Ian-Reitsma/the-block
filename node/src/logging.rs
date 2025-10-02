use crypto_suite::hashing::blake3;

/// Derive a short correlation ID from a block height.
pub fn corr_id_height(height: u64) -> String {
    format!("{:016x}", height)
}

/// Derive a short correlation ID from a hash value.
pub fn corr_id_hash(hash: &[u8]) -> String {
    let h = blake3::hash(hash);
    hex::encode(&h.as_bytes()[0..8])
}

/// Generate a random correlation identifier for ad-hoc requests.
pub fn corr_id_random() -> String {
    use rand::{rngs::OsRng, RngCore};
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[macro_export]
macro_rules! log_context {
    (block = $height:expr) => {
        tracing::info_span!("block", correlation_id = %$crate::logging::corr_id_height($height))
    };
    (tx = $hash:expr) => {
        tracing::info_span!("tx", correlation_id = %$crate::logging::corr_id_hash(&$hash))
    };
    (request) => {
        tracing::info_span!(
            "request",
            correlation_id = %$crate::logging::corr_id_random()
        )
    };
    (request = $id:expr) => {
        tracing::info_span!("request", correlation_id = %$id)
    };
    (provider = $id:expr) => {
        tracing::info_span!("provider", id = %$id)
    };
}
