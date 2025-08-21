use std::collections::HashMap;

/// Simple in-memory registry mapping `@handle` strings to account addresses.
/// Handles are case-sensitive and must be unique.
#[derive(Default)]
pub struct HandleRegistry {
    map: HashMap<String, String>,
}

impl HandleRegistry {
    /// Register a handle for an address.
    /// Returns `false` if the handle already exists.
    pub fn register(&mut self, handle: String, addr: String) -> bool {
        use std::collections::hash_map::Entry;
        match self.map.entry(handle) {
            Entry::Vacant(v) => {
                v.insert(addr);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// Resolve a handle to an address.
    pub fn resolve(&self, handle: &str) -> Option<&String> {
        self.map.get(handle)
    }
}
