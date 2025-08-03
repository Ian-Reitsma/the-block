#[macro_export]
macro_rules! consensus {
    ($vis:vis const $name:ident : $ty:ty = $expr:expr; $reason:expr) => {
        #[doc = $reason]
        $vis const $name: $ty = $expr;
    };
}

use crate::hash_genesis;

consensus!(pub const CHAIN_ID: u32 = 1; "chain identifier for signatures");
consensus!(pub const TX_VERSION: u8 = 2; "transaction version byte");
consensus!(pub const FEE_SPEC_VERSION: u32 = 2; "fee specification version");
consensus!(
    pub const GENESIS_HASH: &str =
        "92fc0fbacb748ac4b7bb561b677ab24bc5561e8e61d406728b90490d56754167";
    "hard-coded genesis block hash"
);

#[allow(dead_code)]
const fn assert_genesis_hash() {
    let a = GENESIS_HASH.as_bytes();
    let b = hash_genesis::calculate_genesis_hash().as_bytes();
    if a.len() != b.len() {
        panic!("GENESIS_HASH length mismatch");
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            panic!("GENESIS_HASH mismatch");
        }
        i += 1;
    }
}
const _: () = assert_genesis_hash();

use bincode::Options;
use once_cell::sync::Lazy;

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
