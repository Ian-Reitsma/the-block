//! Named codec profiles that wrap the canonical configurations used across the
//! workspace.

use super::{BinaryConfig, BinaryProfile, Codec, JsonProfile};

/// Transaction serialization helpers.
pub mod transaction {
    use super::{BinaryConfig, BinaryProfile, Codec};

    /// Canonical codec wrapper for transaction payloads.
    #[must_use]
    pub fn codec() -> Codec {
        Codec::Binary(BinaryProfile::Transaction)
    }

    /// Fetch the canonical binary configuration for transactions.
    #[must_use]
    pub fn config() -> BinaryConfig {
        BinaryProfile::Transaction.config()
    }

    /// Return the canonical profile identifier for transactions.
    #[must_use]
    pub const fn profile() -> BinaryProfile {
        BinaryProfile::Transaction
    }
}

/// Gossip serialization helpers.
pub mod gossip {
    use super::{BinaryConfig, BinaryProfile, Codec};

    /// Canonical codec wrapper for gossip relay persistence.
    #[must_use]
    pub fn codec() -> Codec {
        Codec::Binary(BinaryProfile::Gossip)
    }

    /// Fetch the canonical binary configuration for gossip persistence.
    #[must_use]
    pub fn config() -> BinaryConfig {
        BinaryProfile::Gossip.config()
    }

    /// Return the canonical profile identifier for gossip payloads.
    #[must_use]
    pub const fn profile() -> BinaryProfile {
        BinaryProfile::Gossip
    }
}

/// Storage manifest serialization helpers.
pub mod storage_manifest {
    use super::{BinaryConfig, BinaryProfile, Codec};

    /// Canonical codec wrapper for storage manifest persistence.
    #[must_use]
    pub fn codec() -> Codec {
        Codec::Binary(BinaryProfile::StorageManifest)
    }

    /// Fetch the canonical binary configuration for storage manifests.
    #[must_use]
    pub fn config() -> BinaryConfig {
        BinaryProfile::StorageManifest.config()
    }

    /// Return the canonical profile identifier for storage manifests.
    #[must_use]
    pub const fn profile() -> BinaryProfile {
        BinaryProfile::StorageManifest
    }
}

/// JSON serialization helpers.
pub mod json {
    use super::{Codec, JsonProfile};

    /// Canonical codec wrapper for JSON payloads.
    #[must_use]
    pub fn codec() -> Codec {
        Codec::Json(JsonProfile::Canonical)
    }

    /// Return the canonical JSON profile identifier.
    #[must_use]
    pub const fn profile() -> JsonProfile {
        JsonProfile::Canonical
    }
}

/// Binary serialization helpers.
pub mod binary {
    use super::{BinaryProfile, Codec};

    /// Canonical codec wrapper for binary payloads.
    #[must_use]
    pub fn codec() -> Codec {
        Codec::Binary(BinaryProfile::Canonical)
    }

    /// Return the canonical binary profile identifier.
    #[must_use]
    pub const fn profile() -> BinaryProfile {
        BinaryProfile::Canonical
    }
}

pub use binary::codec as binary;
pub use gossip::codec as gossip;
pub use json::codec as json;
pub use storage_manifest::codec as storage_manifest;
pub use transaction::codec as transaction;

pub use gossip::config as gossip_config;
pub use storage_manifest::config as storage_manifest_config;
pub use transaction::config as transaction_config;
