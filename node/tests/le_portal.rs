use tempfile::tempdir;
use the_block::le_portal::{list_requests, record_canary, record_request};

#[test]
fn log_and_list_le_request() {
    let dir = tempdir().unwrap();
    let base = dir.path().to_str().unwrap();
    let hash = record_request(base, "Agency", "case123", "US").unwrap();
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
