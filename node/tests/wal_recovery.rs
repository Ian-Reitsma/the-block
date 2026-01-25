#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]

use std::fs;
use the_block::SimpleDb;

#[path = "util/temp.rs"]
mod temp;
use temp::temp_dir;

#[test]
fn wal_recovers_unflushed_ops() {
    let dir = temp_dir("wal_db");
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("k", b"v".to_vec());
        // Intentionally omit flush to simulate crash
    }
    let db2 = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db2.get("k"), Some(b"v".to_vec()));
}

#[test]
fn wal_replays_once_after_compaction_crash() {
    let dir = temp_dir("wal_compact_crash");
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("k", b"v".to_vec());
        db.flush();
    }
    let wal_path = dir.path().join("default").join("wal.log");
    let db = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db.get("k"), Some(b"v".to_vec()));
    if wal_path.exists() {
        assert_eq!(fs::metadata(&wal_path).unwrap().len(), 0);
    }
    let db2 = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db2.get("k"), Some(b"v".to_vec()));
    if wal_path.exists() {
        assert_eq!(fs::metadata(&wal_path).unwrap().len(), 0);
    }
}
