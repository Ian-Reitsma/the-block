#![cfg(feature = "integration-tests")]
use std::fs;
use std::path::PathBuf;

/// Record the RNG `seed` under `target/test-seeds/<name>.seed` for replay.
#[allow(dead_code)]
pub fn record_seed(name: &str, seed: u64) {
    let dir = std::env::var("TB_TEST_SEED_DIR").unwrap_or_else(|_| "target/test-seeds".into());
    let path: PathBuf = [dir.as_str(), &format!("{name}.seed")].iter().collect();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, seed.to_string());
}
