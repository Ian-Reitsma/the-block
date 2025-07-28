pub const CHAIN_ID: u32 = 1;
pub const TX_VERSION: u8 = 1;

use bincode::Options;
use once_cell::sync::Lazy;

pub fn domain_tag() -> &'static [u8] {
    static TAG: Lazy<[u8; 16]> = Lazy::new(|| {
        let mut buf = [0u8; 16];
        let prefix = b"THE_BLOCKv1|"; // 12 bytes
        buf[..prefix.len()].copy_from_slice(prefix);
        buf[prefix.len()..prefix.len() + 4].copy_from_slice(&CHAIN_ID.to_le_bytes());
        buf
    });
    &*TAG
}

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
