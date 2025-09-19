#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]

use std::fs;
use tempfile::tempdir;
use the_block::redact_at_rest;

#[test]
fn redact_delete_and_hash() {
    let dir = tempdir().unwrap();
    let path_del = dir.path().join("del.log");
    fs::write(&path_del, "secret").unwrap();
    redact_at_rest(dir.path().to_str().unwrap(), 0, false).unwrap();
    assert!(!path_del.exists());

    let path_hash = dir.path().join("hash.log");
    fs::write(&path_hash, "secret").unwrap();
    redact_at_rest(dir.path().to_str().unwrap(), 0, true).unwrap();
    let content = fs::read_to_string(&path_hash).unwrap();
    assert_eq!(64, content.len());
    assert_ne!(content, "secret");
}
