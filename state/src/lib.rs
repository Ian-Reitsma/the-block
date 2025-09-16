//! Merkle state trie and snapshot utilities.
#![forbid(unsafe_code)]

pub mod audit;
pub mod contracts;
pub mod did;
pub mod difficulty_history;
pub mod partition_log;
pub mod schema;
pub mod snapshot;
pub mod trie;

pub use audit::append as append_audit;
pub use contracts::{ContractId, ContractStore};
pub use did::{DidState, DidStateError};
pub use difficulty_history::{append as append_difficulty, recent as recent_difficulty};
pub use partition_log::{append as append_partition, PartitionRecord};
pub use schema::{migrate, SCHEMA_VERSION};
pub use snapshot::{Snapshot, SnapshotManager};
pub use trie::{MerkleTrie, Proof};
