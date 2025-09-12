//! Merkle state trie and snapshot utilities.
#![forbid(unsafe_code)]

pub mod contracts;
pub mod schema;
pub mod snapshot;
pub mod trie;

pub use contracts::{ContractId, ContractStore};
pub use schema::{migrate, SCHEMA_VERSION};
pub use snapshot::{Snapshot, SnapshotManager};
pub use trie::{MerkleTrie, Proof};
