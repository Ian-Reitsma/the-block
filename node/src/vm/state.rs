use std::collections::HashMap;

pub type ContractId = u64;

/// In-memory contract storage. Real implementation should use persistent DB.
#[derive(Default)]
pub struct State {
    next_id: ContractId,
    storage: HashMap<ContractId, Vec<u8>>,
}

impl State {
    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        let id = self.next_id;
        self.next_id += 1;
        self.storage.insert(id, code);
        id
    }

    pub fn code(&self, id: ContractId) -> Option<&Vec<u8>> {
        self.storage.get(&id)
    }

    pub fn set_storage(&mut self, id: ContractId, data: Vec<u8>) {
        self.storage.insert(id, data);
    }
}
