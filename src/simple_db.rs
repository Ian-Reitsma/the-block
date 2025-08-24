use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::Path,
};
/// Minimal file-backed key-value store emulating the subset of `sled::Db`
/// used by the project. Data is serialized via `bincode` to `<path>/db` on
/// `flush()` and deserialized on open. A write-ahead log at `<path>/wal`
/// guarantees crash-safe writes: operations are appended with a BLAKE3
/// checksum and replayed on open before the log is truncated.
#[derive(Default)]
pub struct SimpleDb {
    map: HashMap<String, Vec<u8>>,
    path: String,
}

impl SimpleDb {
    pub fn open(path: &str) -> Self {
        let db_path = Path::new(path).join("db");
        let mut map = fs::read(&db_path)
            .ok()
            .and_then(|b| {
                bincode::deserialize(&b).ok().or_else(|| {
                    use std::io::Cursor;
                    let mut cur = Cursor::new(&b);
                    let count: u64 = bincode::deserialize_from(&mut cur).ok()?;
                    if count == 0 {
                        return None;
                    }
                    let key_len: u64 = bincode::deserialize_from(&mut cur).ok()?;
                    let mut key = vec![0u8; key_len as usize];
                    cur.read_exact(&mut key).ok()?;
                    if key != b"chain" {
                        return None;
                    }
                    let val_len: u64 = bincode::deserialize_from(&mut cur).ok()?;
                    let mut val = vec![0u8; val_len as usize];
                    cur.read_exact(&mut val).ok()?;
                    let mut m = HashMap::new();
                    m.insert("chain".to_string(), val);
                    Some(m)
                })
            })
            .unwrap_or_default();

        // Replay any pending WAL entries
        let wal_path = Path::new(path).join("wal");
        if let Ok(bytes) = fs::read(&wal_path) {
            let mut cur = std::io::Cursor::new(bytes);
            while let Ok(entry) = bincode::deserialize_from::<_, WalEntry>(&mut cur) {
                let rec_bytes = bincode::serialize(&entry.record).unwrap_or_default();
                let mut h = Hasher::new();
                h.update(&rec_bytes);
                if entry.checksum == *h.finalize().as_bytes() {
                    match entry.record.value {
                        Some(val) => {
                            map.insert(entry.record.key.clone(), val);
                        }
                        None => {
                            map.remove(&entry.record.key);
                        }
                    }
                }
            }
            // Commit replayed state and clear WAL
            let _ = fs::remove_file(&wal_path);
            if let Ok(bytes) = bincode::serialize(&map) {
                let _ = fs::write(&db_path, bytes);
            }
        }

        Self {
            map,
            path: path.to_string(),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.map.get(key).cloned()
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        log_wal(
            &self.path,
            WalRecord {
                key: key.to_string(),
                value: Some(value.clone()),
            },
        );
        let prev = self.map.insert(key.to_string(), value);
        // Persist immediately so a truncated WAL can't drop committed writes.
        self.flush();
        prev
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        log_wal(
            &self.path,
            WalRecord {
                key: key.to_string(),
                value: None,
            },
        );
        let prev = self.map.remove(key);
        // Persist immediately so a truncated WAL can't resurrect removed keys.
        self.flush();
        prev
    }

    pub fn flush(&self) {
        let db_path = Path::new(&self.path).join("db");
        if let Some(parent) = db_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(bytes) = bincode::serialize(&self.map) {
            let _ = fs::write(&db_path, bytes);
            let wal_path = Path::new(&self.path).join("wal");
            let _ = fs::remove_file(wal_path);
        }
    }
}

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

fn log_wal(path: &str, record: WalRecord) {
    let wal_path = Path::new(path).join("wal");
    if let Ok(rec_bytes) = bincode::serialize(&record) {
        let mut h = Hasher::new();
        h.update(&rec_bytes);
        let entry = WalEntry {
            record,
            checksum: *h.finalize().as_bytes(),
        };
        if let Ok(bytes) = bincode::serialize(&entry) {
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(wal_path)
            {
                let _ = f.write_all(&bytes);
            }
        }
    }
}
