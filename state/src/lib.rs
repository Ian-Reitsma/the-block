//! Merkle state trie and snapshot utilities.
#![forbid(unsafe_code)]

pub mod snapshot;
pub mod trie;

pub use snapshot::{Snapshot, SnapshotManager};
pub use trie::{MerkleTrie, Proof};
