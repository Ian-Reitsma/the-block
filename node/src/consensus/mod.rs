#[macro_export]
macro_rules! consensus {
    ($vis:vis const $name:ident : $ty:ty = $expr:expr, $reason:expr) => {
        #[doc = $reason]
        $vis const $name: $ty = $expr;
    };
}

pub mod constants;
pub mod difficulty;
pub mod difficulty_retune;
pub mod engine;
pub mod finality;
pub mod fork_choice;
pub mod hadamard;
pub mod leader;
#[cfg(feature = "telemetry")]
pub mod observer;
pub mod pos;
pub mod pow;
pub mod unl;
pub mod vdf;
pub mod committee {
    pub mod topk;
}

use crate::hash_genesis;

consensus!(
    pub const CHAIN_ID: u32 = 1,
    "chain identifier for signatures"
);
consensus!(pub const TX_VERSION: u8 = 2, "transaction version byte");
consensus!(
    pub const FEE_SPEC_VERSION: u32 = 2,
    "fee specification version"
);
consensus!(
    pub const GENESIS_HASH: &str =
        "07a02d78d9b62d7fe4a32840386017fd4fba605d1c3e54b2adcf09fd91d8618d",
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

use codec::{profiles, BinaryConfig};
use std::sync::LazyLock;

use crypto_suite::transactions::domain_tag_for as suite_domain_tag_for;

/// Compute the 16-byte domain separation tag for a given chain ID.
pub fn domain_tag_for(id: u32) -> [u8; 16] {
    suite_domain_tag_for(id).into()
}

/// Returns the 16-byte domain separation tag used in all signing operations.
pub fn domain_tag() -> &'static [u8] {
    static TAG: LazyLock<[u8; 16]> = LazyLock::new(|| domain_tag_for(CHAIN_ID));
    &*TAG
}

/// Canonical binary configuration shared across the codebase.
pub fn binary_config() -> BinaryConfig {
    profiles::transaction::config()
}
