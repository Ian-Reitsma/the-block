use std::{collections::HashMap, fs, io::Read, path::Path};
/// Minimal file-backed key-value store emulating the subset of `sled::Db`
/// used by the project. Data is serialized via `bincode` to `<path>/db` on
/// `flush()` and deserialized on open. This is sufficient for tests that rely
/// on snapshotting the database to disk.
#[derive(Default)]
pub struct SimpleDb {
    map: HashMap<String, Vec<u8>>,
    path: String,
}

impl SimpleDb {
    pub fn open(path: &str) -> Self {
        let db_path = Path::new(path).join("db");
        let map = fs::read(&db_path)
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
        Self {
            map,
            path: path.to_string(),
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.map.get(key).cloned()
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.map.insert(key.to_string(), value)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.map.remove(key)
    }

    pub fn flush(&self) {
        let db_path = Path::new(&self.path).join("db");
        if let Some(parent) = db_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(bytes) = bincode::serialize(&self.map) {
            let _ = fs::write(db_path, bytes);
        }
    }
}
