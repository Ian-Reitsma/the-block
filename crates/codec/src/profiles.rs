//! Named codec profiles that wrap the canonical configurations used across the
//! workspace.

use super::{BincodeConfig, BincodeProfile, CborProfile, Codec, JsonProfile};

/// Bincode codec configured for transaction payloads.
#[must_use]
pub fn transaction() -> Codec {
    Codec::Bincode(BincodeProfile::Transaction)
}

/// Canonical bincode configuration for transaction payloads.
#[must_use]
pub fn transaction_config() -> BincodeConfig {
    BincodeProfile::Transaction.config()
}

/// Bincode codec configured for gossip relay persistence.
#[must_use]
pub fn gossip() -> Codec {
    Codec::Bincode(BincodeProfile::Gossip)
}

/// Canonical bincode configuration for gossip relay persistence.
#[must_use]
pub fn gossip_config() -> BincodeConfig {
    BincodeProfile::Gossip.config()
}

/// Bincode codec configured for storage manifest persistence.
#[must_use]
pub fn storage_manifest() -> Codec {
    Codec::Bincode(BincodeProfile::StorageManifest)
}

/// Canonical bincode configuration for storage manifest persistence.
#[must_use]
pub fn storage_manifest_config() -> BincodeConfig {
    BincodeProfile::StorageManifest.config()
}

/// Canonical JSON codec configuration.
#[must_use]
pub fn json() -> Codec {
    Codec::Json(JsonProfile::Canonical)
}

/// Canonical CBOR codec configuration.
#[must_use]
pub fn cbor() -> Codec {
    Codec::Cbor(CborProfile::Canonical)
}
