#![forbid(unsafe_code)]

pub mod encryption;
pub mod hashing;
pub mod key_derivation;
pub mod mac;
pub mod signatures;
pub mod transactions;
pub mod zk;

/// Semantic version of the crypto suite crate for telemetry labeling.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use transactions::{
    canonical_bincode_config, canonical_payload_bytes, domain_separated_message, domain_tag_for,
    try_canonical_payload_bytes, DomainTag, TransactionError, TransactionSigner,
    TRANSACTION_DOMAIN_PREFIX,
};
