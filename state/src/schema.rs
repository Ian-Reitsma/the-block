//! Schema versioning and migration utilities for state persistence.
#![forbid(unsafe_code)]

pub const SCHEMA_VERSION: u32 = 4;
const KEY: &str = "__schema_version";

/// Migrate an existing key-value store to schema v4 by bumping the version
/// marker. The store is abstracted via simple `get`/`put` closures to avoid
/// coupling with a specific backend.
pub fn migrate<F, G>(mut get: F, mut put: G)
where
    F: FnMut(&str) -> Option<Vec<u8>>,
    G: FnMut(&str, Vec<u8>),
{
    let current: u32 = get(KEY)
        .and_then(|b| bincode::deserialize(&b).ok())
        .unwrap_or(0);
    if current < SCHEMA_VERSION {
        if let Ok(bytes) = bincode::serialize(&SCHEMA_VERSION) {
            put(KEY, bytes);
        }
    }
}

