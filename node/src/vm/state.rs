use std::path::PathBuf;

use state::ContractStore;

pub type ContractId = u64;

/// Wrapper around `ContractStore` providing contract code and storage persistence.
pub struct State {
    store: ContractStore,
}

impl State {
    /// Create a new in-memory state store.
    pub fn new() -> Self {
        Self {
            store: ContractStore::new(None),
        }
    }

    /// Create a new persistent state store backed by the given path.
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            store: ContractStore::new(Some(path)),
        }
    }

    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        self.store.deploy(code)
    }

    pub fn code(&self, id: ContractId) -> Option<Vec<u8>> {
        self.store.code(id).cloned()
    }

    pub fn set_storage(&mut self, id: ContractId, data: Vec<u8>) {
        self.store.set_state(id, data);
    }

    pub fn storage(&self, id: ContractId) -> Option<Vec<u8>> {
        self.store.state(id)
    }
}
