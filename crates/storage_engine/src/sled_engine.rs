use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use sled::Batch;
use tempfile::TempDir;

use crate::{
    KeyValue, KeyValueBatch, KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};

pub struct SledEngine {
    _temp_dir: Option<TempDir>,
    db: sled::Db,
    byte_limit: Mutex<Option<usize>>,
}

impl SledEngine {
    fn open_internal<P: AsRef<Path>>(path: P, temp_dir: Option<TempDir>) -> StorageResult<Self> {
        let db = sled::open(path.as_ref()).map_err(StorageError::from)?;
        Ok(Self {
            _temp_dir: temp_dir,
            db,
            byte_limit: Mutex::new(None),
        })
    }

    fn tree(&self, cf: &str) -> StorageResult<sled::Tree> {
        self.db.open_tree(cf).map_err(StorageError::from)
    }

    fn enforce_limit(&self, len: usize) -> StorageResult<()> {
        if let Some(limit) = *self.byte_limit.lock().unwrap_or_else(|e| e.into_inner()) {
            if len > limit {
                return Err(StorageError::backend("byte limit exceeded"));
            }
        }
        Ok(())
    }
}

impl KeyValue for SledEngine {
    type Batch = SledBatch;
    type Iter = SledIterator;

    fn open(path: &str) -> StorageResult<Self> {
        Self::open_internal(path, None)
    }

    fn flush_wal(&self) -> StorageResult<()> {
        self.db.flush().map(|_| ()).map_err(StorageError::from)
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        let _ = self.tree(cf)?;
        Ok(())
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let tree = self.tree(cf)?;
        Ok(tree
            .get(key)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec()))
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.enforce_limit(value.len())?;
        let tree = self.tree(cf)?;
        Ok(tree
            .insert(key, value)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec()))
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.enforce_limit(value.len())?;
        let tree = self.tree(cf)?;
        tree.insert(key, value)
            .map(|_| ())
            .map_err(StorageError::from)
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let tree = self.tree(cf)?;
        Ok(tree
            .remove(key)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec()))
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        let tree = self.tree(cf)?;
        let mut items = Vec::new();
        for entry in tree.scan_prefix(prefix) {
            let (k, v) = entry.map_err(StorageError::from)?;
            items.push((k.to_vec(), v.to_vec()));
        }
        Ok(SledIterator { items, index: 0 })
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        let mut names: Vec<String> = self
            .db
            .tree_names()
            .into_iter()
            .filter_map(|n| String::from_utf8(n.to_vec()).ok())
            .collect();
        if !names.iter().any(|n| n == "default") {
            names.push("default".to_string());
        }
        Ok(names)
    }

    fn make_batch(&self) -> Self::Batch {
        SledBatch { ops: Vec::new() }
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        let mut grouped: HashMap<String, Batch> = HashMap::new();
        for op in batch.ops {
            let entry = grouped.entry(op.cf.clone()).or_insert_with(Batch::default);
            match op.kind {
                SledBatchKind::Put { key, value } => {
                    self.enforce_limit(value.len())?;
                    entry.insert(key, value);
                }
                SledBatchKind::Delete { key } => {
                    entry.remove(key);
                }
            }
        }
        for (cf, ops) in grouped.into_iter() {
            let tree = self.tree(&cf)?;
            tree.apply_batch(ops).map_err(StorageError::from)?;
        }
        Ok(())
    }

    fn flush(&self) -> StorageResult<()> {
        self.db.flush().map(|_| ()).map_err(StorageError::from)
    }

    fn compact(&self) -> StorageResult<()> {
        Ok(())
    }

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()> {
        let mut guard = self.byte_limit.lock().unwrap_or_else(|e| e.into_inner());
        *guard = limit;
        Ok(())
    }

    fn metrics(&self) -> StorageResult<StorageMetrics> {
        let size_on_disk_bytes = self.db.size_on_disk().map_err(StorageError::from)?;

        Ok(StorageMetrics {
            backend: "sled",
            size_on_disk_bytes: Some(size_on_disk_bytes),
            ..StorageMetrics::default()
        })
    }
}

pub struct SledIterator {
    items: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl KeyValueIterator for SledIterator {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>> {
        if self.index >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[self.index].clone();
        self.index += 1;
        Ok(Some(item))
    }
}

struct SledBatchEntry {
    cf: String,
    kind: SledBatchKind,
}

enum SledBatchKind {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

pub struct SledBatch {
    ops: Vec<SledBatchEntry>,
}

impl KeyValueBatch for SledBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.ops.push(SledBatchEntry {
            cf: cf.to_string(),
            kind: SledBatchKind::Put {
                key: key.to_vec(),
                value: value.to_vec(),
            },
        });
        Ok(())
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        self.ops.push(SledBatchEntry {
            cf: cf.to_string(),
            kind: SledBatchKind::Delete { key: key.to_vec() },
        });
        Ok(())
    }
}

impl Default for SledEngine {
    fn default() -> Self {
        let dir = tempfile::tempdir().expect("tmpdb");
        let path = dir.path().to_path_buf();
        Self::open_internal(path, Some(dir)).expect("open temp sled engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyValue;
    use tempfile::tempdir;

    #[test]
    fn put_and_delete_values() {
        let dir = tempdir().expect("temp dir");
        let engine =
            SledEngine::open(dir.path().to_str().expect("path")).expect("open sled engine");

        engine.put("default", b"foo", b"bar").expect("insert value");
        assert_eq!(
            engine.get("default", b"foo").unwrap(),
            Some(b"bar".to_vec())
        );

        engine.delete("default", b"foo").expect("delete value");
        assert!(engine.get("default", b"foo").unwrap().is_none());
    }
}
