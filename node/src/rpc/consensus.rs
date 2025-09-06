use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::Blockchain;

/// Return the current PoW difficulty.
pub fn difficulty(bc: &Arc<Mutex<Blockchain>>) -> Value {
    let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
    serde_json::json!({"difficulty": guard.difficulty})
}
