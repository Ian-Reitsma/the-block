pub use crate::consensus::*;

/// Maximum pending blob bytes allowed in the mempool before new blob
/// submissions are rejected.
pub const MAX_UNFINALIZED_BLOB_BYTES: u64 = 32 * 1024 * 1024; // 32 MB
