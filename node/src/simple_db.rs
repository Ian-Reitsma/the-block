use crate::storage::fs::credit_err_to_io;
#[cfg(feature = "telemetry")]
use crate::telemetry::WAL_CORRUPT_RECOVERY_TOTAL;
use blake3::Hasher;
use credits::CreditError;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, io::Write, path::Path};

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

/// Minimal file-backed key-value store.
#[derive(Default)]
pub struct SimpleDb {
    map: HashMap<String, Vec<u8>>,
    path: String,
    next_id: u64,
    byte_limit: Option<usize>,
}

impl SimpleDb {
    pub fn open(path: &str) -> Self {
        let db_path = Path::new(path).join("db");
        let mut map: HashMap<String, Vec<u8>> = fs::read(&db_path)
            .ok()
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_default();
        let last_id = map
            .get("__wal_id")
            .and_then(|b| bincode::deserialize(b).ok())
            .unwrap_or(0u64);
        let wal_path = Path::new(path).join("wal");
        if let Ok(bytes) = fs::read(&wal_path) {
            let mut cur = std::io::Cursor::new(bytes);
            let mut entries = Vec::new();
            while let Ok(entry) = bincode::deserialize_from::<_, WalEntry>(&mut cur) {
                entries.push(entry);
            }
            if matches!(entries.last().map(|e| &e.op), Some(WalOp::End { .. })) {
                let _ = fs::remove_file(&wal_path);
            } else {
                let mut applied = last_id;
                for entry in entries {
                    let op_bytes = bincode::serialize(&entry.op).unwrap_or_default();
                    let mut h = Hasher::new();
                    h.update(&op_bytes);
                    if entry.checksum != *h.finalize().as_bytes() {
                        #[cfg(feature = "telemetry")]
                        WAL_CORRUPT_RECOVERY_TOTAL.inc();
                        continue;
                    }
                    if let WalOp::Record(rec) = entry.op {
                        if rec.id > applied {
                            match rec.value {
                                Some(val) => {
                                    map.insert(rec.key, val);
                                }
                                None => {
                                    map.remove(&rec.key);
                                }
                            }
                            applied = rec.id;
                        }
                    }
                }
                let _ = fs::remove_file(&wal_path);
                map.insert("__wal_id".into(), bincode::serialize(&applied).unwrap());
                if let Ok(bytes) = bincode::serialize(&map) {
                    let _ = fs::write(&db_path, bytes);
                }
            }
        }
        Self {
            map,
            path: path.to_string(),
            next_id: last_id + 1,
            byte_limit: None,
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.map.get(key).cloned()
    }

    pub fn try_insert(&mut self, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        let id = self.next_id;
        self.next_id += 1;
        log_wal(
            &self.path,
            WalOp::Record(WalRecord {
                key: key.to_string(),
                value: Some(value.clone()),
                id,
            }),
        )?;
        self.map
            .insert("__wal_id".into(), bincode::serialize(&id).unwrap());
        let prev = self.map.insert(key.to_string(), value);
        self.try_flush()?;
        Ok(prev)
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.try_insert(key, value).expect("db insert")
    }

    pub fn try_remove(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let id = self.next_id;
        self.next_id += 1;
        log_wal(
            &self.path,
            WalOp::Record(WalRecord {
                key: key.to_string(),
                value: None,
                id,
            }),
        )?;
        self.map
            .insert("__wal_id".into(), bincode::serialize(&id).unwrap());
        let prev = self.map.remove(key);
        self.try_flush()?;
        Ok(prev)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.try_remove(key).expect("db remove")
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.map
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub fn try_flush(&self) -> io::Result<()> {
        let db_path = Path::new(&self.path).join("db");
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = bincode::serialize(&self.map).unwrap();
        if let Some(limit) = self.byte_limit {
            if bytes.len() > limit {
                return Err(credit_err_to_io(CreditError::Insufficient));
            }
        }
        fs::write(&db_path, &bytes)?;
        let wal_path = Path::new(&self.path).join("wal");
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;
        let last = self
            .map
            .get("__wal_id")
            .and_then(|b| bincode::deserialize(b).ok())
            .unwrap_or(0);
        let op = WalOp::End { last_id: last };
        let op_bytes = bincode::serialize(&op).unwrap();
        let mut h = Hasher::new();
        h.update(&op_bytes);
        let entry = WalEntry {
            op,
            checksum: *h.finalize().as_bytes(),
        };
        let entry_bytes = bincode::serialize(&entry).unwrap();
        f.write_all(&entry_bytes)?;
        let _ = fs::remove_file(&wal_path);
        Ok(())
    }

    pub fn flush(&self) {
        let _ = self.try_flush();
    }

    pub fn set_byte_limit(&mut self, limit: usize) {
        self.byte_limit = Some(limit);
    }
}

fn log_wal(path: &str, op: WalOp) -> io::Result<()> {
    let wal_path = Path::new(path).join("wal");
    if let Some(parent) = wal_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let op_bytes = bincode::serialize(&op).unwrap();
    let mut h = Hasher::new();
    h.update(&op_bytes);
    let entry = WalEntry {
        op,
        checksum: *h.finalize().as_bytes(),
    };
    let bytes = bincode::serialize(&entry).unwrap();
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(wal_path)?;
    f.write_all(&bytes)?;
    Ok(())
}
