#![cfg(feature = "integration-tests")]
use crypto_suite::hashing::blake3;
use std::fs::File;
use std::io::Write;

#[test]
fn detects_tampered_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bin");
    let mut f = File::create(&path).unwrap();
    write!(f, "hello").unwrap();
    let expected = blake3::hash(b"hello").to_hex().to_string();
    assert!(the_block::provenance::verify_file(&path, &expected));

    let mut f = File::create(&path).unwrap();
    write!(f, "bye").unwrap();
    assert!(!the_block::provenance::verify_file(&path, &expected));
}
