use crate::hashlayout::{BlockEncoder, ZERO_HASH};

/// Returns the canonical genesis hash at compile time.
pub const fn calculate_genesis_hash() -> &'static str {
    BlockEncoder {
        index: 0,
        prev: ZERO_HASH,
        timestamp: 0,
        nonce: 0,
        difficulty: 8,
        coin_c: 0,
        coin_i: 0,
        storage_sub: 0,
        read_sub: 0,
        compute_sub: 0,
        storage_sub_it: 0,
        read_sub_it: 0,
        compute_sub_it: 0,
        read_root: [0; 32],
        fee_checksum: ZERO_HASH,
        state_root: ZERO_HASH,
        tx_ids: &[],
        l2_roots: &[],
        l2_sizes: &[],
        vdf_commit: [0; 32],
        vdf_output: [0; 32],
        vdf_proof: &[],
    }
    .const_hash()
}

/// Runtime helper used by tests and tooling.
#[cfg(test)]
pub fn calculate_genesis_hash_runtime() -> String {
    BlockEncoder {
        index: 0,
        prev: ZERO_HASH,
        timestamp: 0,
        nonce: 0,
        difficulty: 8,
        coin_c: 0,
        coin_i: 0,
        storage_sub: 0,
        read_sub: 0,
        compute_sub: 0,
        storage_sub_it: 0,
        read_sub_it: 0,
        compute_sub_it: 0,
        read_root: [0; 32],
        fee_checksum: ZERO_HASH,
        state_root: ZERO_HASH,
        tx_ids: &[],
        l2_roots: &[],
        l2_sizes: &[],
        vdf_commit: [0; 32],
        vdf_output: [0; 32],
        vdf_proof: &[],
    }
    .hash()
}
