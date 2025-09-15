use blake3::hash;
use std::fs;
use std::path::Path;

#[cfg(feature = "telemetry")]
use crate::telemetry::{BUILD_PROVENANCE_INVALID_TOTAL, BUILD_PROVENANCE_VALID_TOTAL};

/// Verify the hash of the current executable against the embedded build hash.
pub fn verify_self() -> bool {
    let expected = env!("BUILD_BIN_HASH");
    if let Ok(path) = std::env::current_exe() {
        let ok = verify_file(&path, expected);
        if ok {
            #[cfg(feature = "telemetry")]
            BUILD_PROVENANCE_VALID_TOTAL.inc();
        } else {
            #[cfg(feature = "telemetry")]
            BUILD_PROVENANCE_INVALID_TOTAL.inc();
        }
        ok
    } else {
        #[cfg(feature = "telemetry")]
        BUILD_PROVENANCE_INVALID_TOTAL.inc();
        false
    }
}

/// Verify that `path` hashes to `expected` (hex).
pub fn verify_file(path: &Path, expected: &str) -> bool {
    match fs::read(path) {
        Ok(bytes) => hash(&bytes).to_hex().to_string() == expected,
        Err(_) => false,
    }
}
