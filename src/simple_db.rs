use std::collections::HashMap;
/// Minimal in-memory key-value store emulating the subset of `sled::Db`
/// used by the project. Data is not persisted across runs.
#[derive(Default)]
pub struct SimpleDb {
    map: HashMap<String, Vec<u8>>,
}

impl SimpleDb {
    pub fn open(_path: &str) -> Self {
        Self {
            map: HashMap::new(),
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

    pub fn flush(&self) {}
}
