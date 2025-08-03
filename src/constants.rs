/// Unique identifier for this chain instance. Embedded in signatures
/// to prevent cross-network replay.
pub const CHAIN_ID: u32 = 1;
/// Transaction version byte hashed into each `SignedTransaction::id`.
pub const TX_VERSION: u8 = 2;

/// Fee specification version for runtime compatibility checks.
pub const FEE_SPEC_VERSION: u32 = 2;

/// Hard-coded hash of the genesis block.
pub const GENESIS_HASH: &str = "92fc0fbacb748ac4b7bb561b677ab24bc5561e8e61d406728b90490d56754167";

use bincode::Options;
use blake3;
use once_cell::sync::Lazy;

/// Recomputes the expected genesis block hash using the same field order as
/// the runtime `calculate_hash`.
pub fn calculate_genesis_hash() -> String {
    let mut hasher = blake3::Hasher::new();
    let index = 0u64;
    let prev = "0".repeat(64);
    let nonce = 0u64;
    let difficulty = 8u64;
    let coin_c = 0u64;
    let coin_i = 0u64;
    let fee_checksum = "0".repeat(64);
    hasher.update(&index.to_le_bytes());
    hasher.update(prev.as_bytes());
    hasher.update(&nonce.to_le_bytes());
    hasher.update(&difficulty.to_le_bytes());
    hasher.update(&coin_c.to_le_bytes());
    hasher.update(&coin_i.to_le_bytes());
    hasher.update(fee_checksum.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Returns the 16-byte domain separation tag used in all signing operations.
pub fn domain_tag() -> &'static [u8] {
    static TAG: Lazy<[u8; 16]> = Lazy::new(|| {
        let mut buf = [0u8; 16];
        let prefix = b"THE_BLOCKv2|"; // 12 bytes
        buf[..prefix.len()].copy_from_slice(prefix);
        buf[prefix.len()..prefix.len() + 4].copy_from_slice(&CHAIN_ID.to_le_bytes());
        buf
    });
    &*TAG
}

/// Canonical bincode configuration shared across the codebase.
pub fn bincode_config() -> bincode::config::WithOtherEndian<
    bincode::config::WithOtherIntEncoding<bincode::DefaultOptions, bincode::config::FixintEncoding>,
    bincode::config::LittleEndian,
> {
    static CFG: Lazy<
        bincode::config::WithOtherEndian<
            bincode::config::WithOtherIntEncoding<
                bincode::DefaultOptions,
                bincode::config::FixintEncoding,
            >,
            bincode::config::LittleEndian,
        >,
    > = Lazy::new(|| {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
    });
    *CFG
}
