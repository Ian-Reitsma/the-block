#![cfg(feature = "integration-tests")]
use sys::tempfile::{Builder, TempDir};

pub fn temp_dir(prefix: &str) -> TempDir {
    Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("create temp dir")
}
