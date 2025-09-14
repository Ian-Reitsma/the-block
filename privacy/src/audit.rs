use std::collections::HashMap;

/// Return true if the audit log contains data for the given user ID.
pub fn has_data(store: &HashMap<String, String>, id: &str) -> bool {
    store.contains_key(id)
}
