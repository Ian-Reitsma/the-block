/// Derive a short correlation ID from a block height.
pub fn corr_id_height(height: u64) -> String {
    format!("{:016x}", height)
}

/// Derive a short correlation ID from a hash value.
pub fn corr_id_hash(hash: &[u8]) -> String {
    let h = blake3::hash(hash);
    hex::encode(&h.as_bytes()[0..8])
}

#[macro_export]
macro_rules! log_context {
    (block = $height:expr) => {
        tracing::info_span!("block", correlation_id = %$crate::logging::corr_id_height($height))
    };
    (tx = $hash:expr) => {
        tracing::info_span!("tx", correlation_id = %$crate::logging::corr_id_hash(&$hash))
    };
}
