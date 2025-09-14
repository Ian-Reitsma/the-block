//! Merkle state trie and snapshot utilities.
#![forbid(unsafe_code)]

pub mod audit;
pub mod contracts;
pub mod schema;
pub mod snapshot;
pub mod trie;
pub mod partition_log;
pub mod difficulty_history;

pub use audit::append as append_audit;
pub use contracts::{ContractId, ContractStore};
pub use schema::{migrate, SCHEMA_VERSION};
pub use snapshot::{Snapshot, SnapshotManager};
pub use trie::{MerkleTrie, Proof};
pub use partition_log::{append as append_partition, PartitionRecord};
pub use difficulty_history::{append as append_difficulty, recent as recent_difficulty};
