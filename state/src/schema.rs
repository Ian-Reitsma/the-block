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
        .and_then(|b| {
            if b.len() == 4 {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&b);
                Some(u32::from_be_bytes(buf))
            } else {
                None
            }
        })
        .unwrap_or(0);
    if current < SCHEMA_VERSION {
        put(KEY, SCHEMA_VERSION.to_be_bytes().to_vec());
    }
}
