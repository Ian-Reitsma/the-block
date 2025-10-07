use std::{convert::TryInto, path::Path};

use storage_engine::{inhouse_engine::InhouseEngine, KeyValue, KeyValueIterator};

const CF: &str = "difficulty_history";

fn open_engine(path: &Path) -> Option<InhouseEngine> {
    let dir = path.to_string_lossy();
    InhouseEngine::open(dir.as_ref()).ok()
}

pub fn append(path: &Path, height: u64, difficulty: u64) {
    if let Some(db) = open_engine(path) {
        if db.ensure_cf(CF).is_ok() {
            let _ = db.put(CF, &height.to_le_bytes(), &difficulty.to_le_bytes());
            let _ = db.flush();
        }
    }
}

pub fn recent(path: &Path, limit: usize) -> Vec<(u64, u64)> {
    let Some(db) = open_engine(path) else {
        return Vec::new();
    };
    if db.ensure_cf(CF).is_err() {
        return Vec::new();
    }
    let mut iter = match db.prefix_iterator(CF, &[]) {
        Ok(iter) => iter,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    while let Ok(Some((k, v))) = iter.next() {
        if k.len() == 8 && v.len() == 8 {
            if let (Ok(height), Ok(diff)) = (
                TryInto::<[u8; 8]>::try_into(k.as_slice()),
                TryInto::<[u8; 8]>::try_into(v.as_slice()),
            ) {
                entries.push((u64::from_le_bytes(height), u64::from_le_bytes(diff)));
            }
        }
    }
    entries.sort_by_key(|(height, _)| *height);
    entries.into_iter().rev().take(limit).collect()
}
