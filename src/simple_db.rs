use std::{collections::HashMap, fs, path::Path};
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
            .and_then(|b| bincode::deserialize(&b).ok())
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
