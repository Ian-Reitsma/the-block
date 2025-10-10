#![forbid(unsafe_code)]

pub mod encoding;
pub mod encryption;
pub mod error;
pub mod hashing;
pub mod key_derivation;
pub mod mac;
pub mod signatures;
pub mod transactions;
pub mod zk;

/// Semantic version of the crypto suite crate for telemetry labeling.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use encoding::hex;
pub use error::{Error, ErrorKind, Result};

pub use transactions::{
    canonical_binary_config, canonical_payload_bytes, domain_separated_message, domain_tag_for,
    try_canonical_payload_bytes, DomainTag, TransactionError, TransactionSigner,
    TRANSACTION_DOMAIN_PREFIX,
};
