use crate::hashlayout::{BlockEncoder, ZERO_HASH};

/// Returns the canonical genesis hash at compile time.
pub const fn calculate_genesis_hash() -> &'static str {
    BlockEncoder {
        index: 0,
        prev: ZERO_HASH,
        nonce: 0,
        difficulty: 8,
        coin_c: 0,
        coin_i: 0,
        fee_checksum: ZERO_HASH,
        tx_ids: &[],
    }
    .const_hash()
}

/// Runtime helper used by tests and tooling.
#[cfg(test)]
pub fn calculate_genesis_hash_runtime() -> String {
    BlockEncoder {
        index: 0,
        prev: ZERO_HASH,
        nonce: 0,
        difficulty: 8,
        coin_c: 0,
        coin_i: 0,
        fee_checksum: ZERO_HASH,
        tx_ids: &[],
    }
    .hash()
}
