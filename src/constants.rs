pub const CHAIN_ID: u32 = 1;

use bincode::Options;
use once_cell::sync::Lazy;

pub fn domain_tag() -> &'static [u8] {
    static TAG: Lazy<Vec<u8>> = Lazy::new(|| {
        let mut v = b"THE_BLOCK|v1|".to_vec();
        v.extend(CHAIN_ID.to_string().as_bytes());
        v.push(b'|');
        v
    });
    &TAG
}

pub fn bincode_config() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .allow_trailing_bytes()
}