#![forbid(unsafe_code)]

//! First-party serialization traits.
//!
//! This crate provides the foundational traits for serialization and deserialization,
//! implementing the same API surface as serde but with a fully first-party implementation.
//!
//! No third-party dependencies - built entirely in-house for complete auditability.

pub mod de;
pub mod ser;

#[doc(inline)]
pub use de::{Deserialize, DeserializeOwned, DeserializeSeed, Deserializer};

#[doc(inline)]
pub use ser::{Serialize, Serializer};

// Re-export for compatibility
pub use de as deserialize;
pub use de::value::IntoDeserializer;
pub use ser as serialize;
