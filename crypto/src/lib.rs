#[cfg(feature = "quantum")]
pub mod dilithium;

pub mod session;

/// Temporary compatibility re-export so downstream crates that previously used
/// `crypto::ed25519::` continue to compile while the migration to the crypto
/// suite finishes. This will be removed once all callers depend on the suite
/// types directly.
pub use crypto_suite::signatures::ed25519;
pub use crypto_suite::signatures::ed25519::{
    KeyEncodingError as Ed25519KeyEncodingError, Signature as Ed25519Signature,
    SignatureError as Ed25519SignatureError, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};

pub use crypto_suite::transactions::{
    domain_separated_message, domain_tag_for as suite_domain_tag_for, DomainTag, TransactionError,
    TransactionSigner, TRANSACTION_DOMAIN_PREFIX,
};
pub use crypto_suite::{
    canonical_bincode_config, canonical_payload_bytes, hashing, key_derivation, signatures,
    transactions, try_canonical_payload_bytes, zk,
};

/// Domain separation tag for Ed25519 transactions.
pub const ED25519_DOMAIN_TAG: &[u8; 16] = b"TX_ED25519______";

/// Domain separation tag for Dilithium transactions.
#[cfg(feature = "quantum")]
pub const DILITHIUM_DOMAIN_TAG: &[u8; 16] = b"TX_DILITHIUM____";
