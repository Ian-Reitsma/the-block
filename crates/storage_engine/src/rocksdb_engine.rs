use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rocksdb::{
    properties, BoundColumnFamily, ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded,
    Options, WriteBatch,
};
use tempfile::TempDir;

use crate::{
    KeyValue, KeyValueBatch, KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};

pub struct RocksDbEngine {
    _temp_dir: Option<TempDir>,
    db: DBWithThreadMode<MultiThreaded>,
    byte_limit: Mutex<Option<usize>>,
    prefix_cache: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    cf_handles: Mutex<HashSet<String>>,
}

impl RocksDbEngine {
    fn open_internal<P: AsRef<Path>>(path: P, temp_dir: Option<TempDir>) -> StorageResult<Self> {
        let path_ref = path.as_ref();
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let existing =
            DBWithThreadMode::<MultiThreaded>::list_cf(&opts, path_ref).unwrap_or_default();
        let mut descriptors: Vec<ColumnFamilyDescriptor> = existing
            .iter()
            .map(|n| ColumnFamilyDescriptor::new(n.to_string(), Options::default()))
            .collect();
        if !existing.iter().any(|n| n == "default") {
            descriptors.push(ColumnFamilyDescriptor::new(
                "default".to_string(),
                Options::default(),
            ));
        }
        let descriptor_names: Vec<String> = descriptors
            .iter()
            .map(|desc| desc.name().to_string())
            .collect();
        let db = DBWithThreadMode::open_cf_descriptors_with_ttl(
            &opts,
            path_ref,
            descriptors,
            Duration::from_secs(24 * 60 * 60),
        )
        .map_err(StorageError::from)?;
        let handles = descriptor_names.into_iter().collect();
        Ok(Self {
            _temp_dir: temp_dir,
            db,
            byte_limit: Mutex::new(None),
            prefix_cache: Mutex::new(HashMap::new()),
            cf_handles: Mutex::new(handles),
        })
    }

    fn enforce_limit(&self, len: usize) -> StorageResult<()> {
        if let Some(limit) = *self.byte_limit.lock() {
            if len > limit {
                return Err(StorageError::backend("byte limit exceeded"));
            }
        }
        Ok(())
    }

    fn handle(&self, name: &str) -> StorageResult<Arc<BoundColumnFamily>> {
        let mut handles = self.cf_handles.lock();
        if !handles.contains(name) {
            self.db
                .create_cf(name, &Options::default())
                .map_err(StorageError::from)?;
            handles.insert(name.to_string());
        }
        drop(handles);
        self.db
            .cf_handle(name)
            .ok_or_else(|| StorageError::backend(format!("missing column family: {name}")))
    }

    fn get_cf(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        if cf == "default" {
            if let Some(v) = self.prefix_cache.lock().get(key) {
                return Ok(Some(v.clone()));
            }
        }
        let handle = self.handle(cf)?;
        let val = self
            .db
            .get_cf(&handle, key)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec());
        if cf == "default" {
            if let Some(ref v) = val {
                self.prefix_cache.lock().insert(key.to_vec(), v.clone());
            }
        }
        Ok(val)
    }
}

impl KeyValue for RocksDbEngine {
    type Batch = RocksDbBatch;
    type Iter = RocksDbIterator;

    fn open(path: &str) -> StorageResult<Self> {
        Self::open_internal(path, None)
    }

    fn flush_wal(&self) -> StorageResult<()> {
        self.db.flush_wal(true).map_err(StorageError::from)
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        let _ = self.handle(cf)?;
        Ok(())
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.get_cf(cf, key)
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.enforce_limit(value.len())?;
        let handle = self.handle(cf)?;
        let prev = self
            .db
            .get_cf(&handle, key)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec());
        self.db
            .put_cf(&handle, key, value)
            .map_err(StorageError::from)?;
        if cf == "default" {
            self.prefix_cache
                .lock()
                .insert(key.to_vec(), value.to_vec());
        }
        Ok(prev)
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.enforce_limit(value.len())?;
        let handle = self.handle(cf)?;
        self.db
            .put_cf(&handle, key, value)
            .map_err(StorageError::from)?;
        if cf == "default" {
            self.prefix_cache
                .lock()
                .insert(key.to_vec(), value.to_vec());
        }
        Ok(())
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let handle = self.handle(cf)?;
        let prev = self
            .db
            .get_cf(&handle, key)
            .map_err(StorageError::from)?
            .map(|v| v.to_vec());
        self.db
            .delete_cf(&handle, key)
            .map_err(StorageError::from)?;
        if cf == "default" {
            self.prefix_cache.lock().remove(key);
        }
        Ok(prev)
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        let handle = self.handle(cf)?;
        let mut items = Vec::new();
        for entry in self.db.prefix_iterator_cf(&handle, prefix) {
            let (k, v) = entry.map_err(StorageError::from)?;
            items.push((k.to_vec(), v.to_vec()));
        }
        Ok(RocksDbIterator { items, index: 0 })
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        Ok(self.cf_handles.lock().iter().cloned().collect())
    }

    fn make_batch(&self) -> Self::Batch {
        RocksDbBatch { ops: Vec::new() }
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        let mut write_batch = WriteBatch::default();
        for op in batch.ops {
            let handle = self.handle(&op.cf)?;
            match op.kind {
                BatchKind::Put { key, value } => {
                    self.enforce_limit(value.len())?;
                    write_batch.put_cf(&handle, &key, &value);
                    if op.cf == "default" {
                        self.prefix_cache.lock().insert(key.clone(), value.clone());
                    }
                }
                BatchKind::Delete { key } => {
                    write_batch.delete_cf(&handle, &key);
                    if op.cf == "default" {
                        self.prefix_cache.lock().remove(&key);
                    }
                }
            }
        }
        self.db.write(write_batch).map_err(StorageError::from)
    }

    fn flush(&self) -> StorageResult<()> {
        self.db.flush().map_err(|e| {
            if e.as_ref().contains("No space") {
                StorageError::backend(e)
            } else {
                StorageError::from(e)
            }
        })
    }

    fn compact(&self) -> StorageResult<()> {
        self.db.compact_range::<&[u8], &[u8]>(None, None);
        Ok(())
    }

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()> {
        *self.byte_limit.lock() = limit;
        Ok(())
    }

    fn metrics(&self) -> StorageResult<StorageMetrics> {
        let pending = self
            .db
            .property_int_value(properties::COMPACTION_PENDING)
            .map_err(StorageError::from)?;
        let running = self
            .db
            .property_int_value(properties::NUM_RUNNING_COMPACTIONS)
            .map_err(StorageError::from)?;
        let total_sst_bytes = self
            .db
            .property_int_value(properties::TOTAL_SST_FILES_SIZE)
            .map_err(StorageError::from)?;
        let memtable_bytes = self
            .db
            .property_int_value(properties::CUR_SIZE_ALL_MEM_TABLES)
            .map_err(StorageError::from)?;
        let level0_files = self
            .db
            .property_int_value(properties::num_files_at_level(0))
            .map_err(StorageError::from)?;

        Ok(StorageMetrics {
            backend: "rocksdb",
            pending_compactions: pending,
            running_compactions: running,
            level0_files,
            total_sst_bytes,
            memtable_bytes,
            size_on_disk_bytes: total_sst_bytes,
        })
    }
}

pub struct RocksDbIterator {
    items: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl KeyValueIterator for RocksDbIterator {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>> {
        if self.index >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[self.index].clone();
        self.index += 1;
        Ok(Some(item))
    }
}

struct RocksDbBatchEntry {
    cf: String,
    kind: BatchKind,
}

enum BatchKind {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

pub struct RocksDbBatch {
    ops: Vec<RocksDbBatchEntry>,
}

impl KeyValueBatch for RocksDbBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.ops.push(RocksDbBatchEntry {
            cf: cf.to_string(),
            kind: BatchKind::Put {
                key: key.to_vec(),
                value: value.to_vec(),
            },
        });
        Ok(())
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        self.ops.push(RocksDbBatchEntry {
            cf: cf.to_string(),
            kind: BatchKind::Delete { key: key.to_vec() },
        });
        Ok(())
    }
}

impl Default for RocksDbEngine {
    fn default() -> Self {
        let dir = tempfile::tempdir().expect("tmpdb");
        let path = dir.path().to_path_buf();
        Self::open_internal(path, Some(dir)).expect("open temp rocksdb engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyValue;
    use tempfile::tempdir;

    #[test]
    fn reuses_cached_column_family_handles() {
        let dir = tempdir().expect("temp dir");
        let engine =
            RocksDbEngine::open(dir.path().to_str().expect("path")).expect("open rocks engine");

        let first = engine.handle("shard:test").expect("first handle");
        let second = engine.handle("shard:test").expect("second handle");

        assert_eq!(Arc::as_ptr(&first), Arc::as_ptr(&second));
    }

    #[test]
    fn flush_and_reopen_preserves_values() {
        let dir = tempdir().expect("temp dir");
        {
            let engine =
                RocksDbEngine::open(dir.path().to_str().expect("path")).expect("open rocks engine");
            engine.put("default", b"foo", b"bar").expect("insert value");
            engine.flush().expect("flush");
        }

        let reopened =
            RocksDbEngine::open(dir.path().to_str().expect("path")).expect("reopen rocks engine");
        assert_eq!(
            reopened.get("default", b"foo").unwrap(),
            Some(b"bar".to_vec())
        );
    }
}
