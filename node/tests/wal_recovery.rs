#![allow(clippy::unwrap_used)]

use the_block::SimpleDb;
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;

mod util;
use util::temp::temp_dir;

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
    #[derive(Serialize, Deserialize)]
    struct WalRecord {
        key: String,
        value: Option<Vec<u8>>,
    }
    #[derive(Serialize, Deserialize)]
    struct WalEntry {
        record: WalRecord,
        checksum: [u8; 32],
    }
    let dir = temp_dir("wal_compact_crash");
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("k", b"v".to_vec());
    }
    let rec = WalRecord {
        key: "k".to_string(),
        value: Some(b"v".to_vec()),
    };
    let rec_bytes = bincode::serialize(&rec).unwrap();
    let mut h = Hasher::new();
    h.update(&rec_bytes);
    let entry = WalEntry {
        record: rec,
        checksum: *h.finalize().as_bytes(),
    };
    let wal_bytes = bincode::serialize(&entry).unwrap();
    let wal_path = dir.path().join("wal");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&wal_path)
        .unwrap();
    f.write_all(&wal_bytes).unwrap();
    let mut map = std::collections::HashMap::new();
    map.insert("k".to_string(), b"v".to_vec());
    let db_bytes = bincode::serialize(&map).unwrap();
    fs::write(dir.path().join("db"), db_bytes).unwrap();
    let db = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db.get("k"), Some(b"v".to_vec()));
    assert!(!wal_path.exists());
    let db2 = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db2.get("k"), Some(b"v".to_vec()));
    assert!(!wal_path.exists());
}
