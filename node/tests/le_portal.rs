#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;
use the_block::le_portal::{
    list_evidence, list_requests, record_canary, record_evidence, record_request,
};

#[test]
fn log_and_list_le_request() {
    let dir = tempdir().unwrap();
    let base = dir.path().to_str().unwrap();
    let hash = record_request(base, "Agency", "case123", "US", "en").unwrap();
    let entries = list_requests(base).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].agency, "Agency");
    assert_eq!(entries[0].case_hash, hash);
    assert_eq!(entries[0].jurisdiction, "US");
}

#[test]
fn canary_hashes_message() {
    let dir = tempdir().unwrap();
    let base = dir.path().to_str().unwrap();
    let hash = record_canary(base, "no requests").unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn evidence_round_trip() {
    let dir = tempdir().unwrap();
    let base = dir.path().to_str().unwrap();
    let data = b"evidence";
    let hash = record_evidence(base, "Agency", "case123", "US", "en", data).unwrap();
    let entries = list_evidence(base).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].evidence_hash, hash);
    assert_eq!(entries[0].case_hash.len(), 64);
}
