#[cfg(feature = "quantum")]
pub mod dilithium;

pub mod session;

/// Domain separation tag for Ed25519 transactions.
pub const ED25519_DOMAIN_TAG: &[u8; 16] = b"TX_ED25519______";

/// Domain separation tag for Dilithium transactions.
#[cfg(feature = "quantum")]
pub const DILITHIUM_DOMAIN_TAG: &[u8; 16] = b"TX_DILITHIUM____";
