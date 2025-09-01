use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type ContractId = u64;

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    next_id: ContractId,
    code: HashMap<ContractId, Vec<u8>>,
    state: HashMap<ContractId, Vec<u8>>,
}

/// Simple on-disk contract store using bincode serialization.
pub struct ContractStore {
    path: Option<PathBuf>,
    data: Persisted,
}

impl ContractStore {
    /// Create a new store optionally backed by a file on disk.
    pub fn new(path: Option<PathBuf>) -> Self {
        let data = path
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_default();
        Self { path, data }
    }

    fn persist(&self) {
        if let Some(ref p) = self.path {
            if let Ok(bytes) = bincode::serialize(&self.data) {
                if let Some(dir) = p.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let _ = fs::write(p, bytes);
            }
        }
    }

    /// Deploy contract code returning a unique identifier.
    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        let id = self.data.next_id;
        self.data.next_id += 1;
        self.data.code.insert(id, code);
        self.persist();
        id
    }

    /// Fetch contract code.
    pub fn code(&self, id: ContractId) -> Option<&Vec<u8>> {
        self.data.code.get(&id)
    }

    /// Fetch contract storage bytes.
    pub fn state(&self, id: ContractId) -> Option<Vec<u8>> {
        self.data.state.get(&id).cloned()
    }

    /// Overwrite contract storage bytes.
    pub fn set_state(&mut self, id: ContractId, data: Vec<u8>) {
        self.data.state.insert(id, data);
        self.persist();
    }
}
