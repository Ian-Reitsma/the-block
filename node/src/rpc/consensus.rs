use std::sync::{Arc, Mutex};

use foundation_serialization::json::Value;

use crate::Blockchain;

/// Return the current PoW difficulty.
pub fn difficulty(bc: &Arc<Mutex<Blockchain>>) -> Value {
    let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
    foundation_serialization::json::json!({"difficulty": guard.difficulty})
}
