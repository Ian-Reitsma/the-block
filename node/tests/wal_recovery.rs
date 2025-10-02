#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]

use crypto_suite::hashing::blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use the_block::SimpleDb;

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
    let dir = temp_dir("wal_compact_crash");
    {
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.insert("k", b"v".to_vec());
    }
    simulate_crash_after_compaction(dir.path());
    let wal_path = dir.path().join("wal");
    let db = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db.get("k"), Some(b"v".to_vec()));
    assert!(!wal_path.exists());
    let db2 = SimpleDb::open(dir.path().to_str().unwrap());
    assert_eq!(db2.get("k"), Some(b"v".to_vec()));
    assert!(!wal_path.exists());
}

#[derive(Serialize, Deserialize)]
struct WalRecord {
    key: String,
    value: Option<Vec<u8>>,
    id: u64,
}

#[derive(Serialize, Deserialize)]
enum WalOp {
    Record(WalRecord),
    End { last_id: u64 },
}

#[derive(Serialize, Deserialize)]
struct WalEntry {
    op: WalOp,
    checksum: [u8; 32],
}

fn simulate_crash_after_compaction(path: &std::path::Path) {
    let wal_path = path.join("wal");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&wal_path)
        .unwrap();
    let rec = WalRecord {
        key: "k".into(),
        value: Some(b"v".to_vec()),
        id: 1,
    };
    let op = WalOp::Record(rec);
    let bytes = bincode::serialize(&op).unwrap();
    let mut h = Hasher::new();
    h.update(&bytes);
    let entry = WalEntry {
        op,
        checksum: *h.finalize().as_bytes(),
    };
    f.write_all(&bincode::serialize(&entry).unwrap()).unwrap();
    let end = WalOp::End { last_id: 1 };
    let bytes = bincode::serialize(&end).unwrap();
    let mut h = Hasher::new();
    h.update(&bytes);
    let entry = WalEntry {
        op: end,
        checksum: *h.finalize().as_bytes(),
    };
    f.write_all(&bincode::serialize(&entry).unwrap()).unwrap();
    let mut map = std::collections::HashMap::new();
    map.insert("k".to_string(), b"v".to_vec());
    map.insert("__wal_id".into(), bincode::serialize(&1u64).unwrap());
    let db_bytes = bincode::serialize(&map).unwrap();
    fs::write(path.join("db"), db_bytes).unwrap();
}
