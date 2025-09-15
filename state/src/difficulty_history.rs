use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use std::path::Path;

pub fn append(path: &Path, height: u64, difficulty: u64) {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let cf = ColumnFamilyDescriptor::new("difficulty_history", Options::default());
    if let Ok(db) = DB::open_cf_descriptors(&opts, path, vec![cf]) {
        if let Some(h) = db.cf_handle("difficulty_history") {
            let _ = db.put_cf(h, &height.to_le_bytes(), &difficulty.to_le_bytes());
        }
    }
}

pub fn recent(path: &Path, limit: usize) -> Vec<(u64, u64)> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let cf = ColumnFamilyDescriptor::new("difficulty_history", Options::default());
    if let Ok(db) = DB::open_cf_descriptors(&opts, path, vec![cf]) {
        if let Some(h) = db.cf_handle("difficulty_history") {
            let mut out = Vec::new();
            let mut iter = db.iterator_cf(h, rocksdb::IteratorMode::End);
            while let Some(Ok((k, v))) = iter.next() {
                if out.len() >= limit {
                    break;
                }
                if k.len() == 8 && v.len() == 8 {
                    let height = u64::from_le_bytes(k.as_ref().try_into().unwrap());
                    let diff = u64::from_le_bytes(v.as_ref().try_into().unwrap());
                    out.push((height, diff));
                }
            }
            return out;
        }
    }
    Vec::new()
}
