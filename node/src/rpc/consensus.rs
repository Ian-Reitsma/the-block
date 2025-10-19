use std::sync::{Arc, Mutex};

use foundation_serialization::Serialize;

use crate::Blockchain;

/// Return the current PoW difficulty.
#[derive(Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DifficultyResponse {
    pub difficulty: u64,
}

pub fn difficulty(bc: &Arc<Mutex<Blockchain>>) -> DifficultyResponse {
    let guard = bc.lock().unwrap_or_else(|e| e.into_inner());
    DifficultyResponse {
        difficulty: guard.difficulty,
    }
}
