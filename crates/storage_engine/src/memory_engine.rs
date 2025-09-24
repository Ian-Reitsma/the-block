use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bincode;
use tempfile::{NamedTempFile, TempDir};

use crate::{
    KeyValue, KeyValueBatch, KeyValueIterator, StorageError, StorageMetrics, StorageResult,
};

#[derive(Default)]
struct Inner {
    column_families: HashMap<String, HashMap<Vec<u8>, Vec<u8>>>,
}

impl Inner {
    fn ensure_cf(&mut self, name: &str) -> &mut HashMap<Vec<u8>, Vec<u8>> {
        self.column_families
            .entry(name.to_string())
            .or_insert_with(HashMap::new)
    }

    fn get_cf(&self, name: &str) -> Option<&HashMap<Vec<u8>, Vec<u8>>> {
        self.column_families.get(name)
    }

    fn ensure_default(&mut self) {
        self.column_families
            .entry("default".to_string())
            .or_insert_with(HashMap::new);
    }
}

/// In-memory fallback database that persists column families to disk using serialized snapshots.
pub struct MemoryEngine {
    path: PathBuf,
    _owned_dir: Option<TempDir>,
    byte_limit: Mutex<Option<usize>>,
    inner: Mutex<Inner>,
}

impl MemoryEngine {
    fn from_path(path: PathBuf, owned_dir: Option<TempDir>) -> StorageResult<Self> {
        let _ = fs::create_dir_all(&path);
        let inner = Mutex::new(Inner {
            column_families: load_column_families(&path).unwrap_or_default(),
        });
        {
            let mut guard = inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.ensure_default();
        }
        Ok(Self {
            path,
            _owned_dir: owned_dir,
            byte_limit: Mutex::new(None),
            inner,
        })
    }

    fn open_from_tempdir(dir: TempDir) -> StorageResult<Self> {
        let path = dir.path().to_path_buf();
        Self::from_path(path, Some(dir))
    }

    fn enforce_limit(&self, len: usize) -> StorageResult<()> {
        if let Some(limit) = *self.byte_limit.lock().unwrap_or_else(|e| e.into_inner()) {
            if len > limit {
                return Err(StorageError::backend("byte limit exceeded"));
            }
        }
        Ok(())
    }

    fn cf_path(&self, cf: &str) -> PathBuf {
        self.path.join(cf_file_name(cf))
    }

    fn legacy_cf_path(&self, cf: &str) -> PathBuf {
        self.path.join(format!("{}.bin", cf.replace(':', "_")))
    }

    fn persist_cf(&self, name: &str, inner: &Inner) -> io::Result<()> {
        let path = self.cf_path(name);
        let legacy_path = self.legacy_cf_path(name);
        let read_only = fs::metadata(&self.path)
            .map(|meta| meta.permissions().readonly())
            .unwrap_or(false);
        if read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("database directory {} is read-only", self.path.display()),
            ));
        }
        if let Some(map) = inner.get_cf(name) {
            if map.is_empty() {
                if path.exists() {
                    fs::remove_file(&path)?;
                }
                if legacy_path != path && legacy_path.exists() {
                    let _ = fs::remove_file(&legacy_path);
                }
            } else {
                let bytes = bincode::serialize(map)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

                let mut temp = NamedTempFile::new_in(&self.path)?;
                temp.as_file_mut().write_all(&bytes)?;
                temp.as_file().sync_all()?;

                let mut backup_path = None;
                if path.exists() {
                    let backup = path.with_extension("bin.old");
                    if backup.exists() {
                        fs::remove_file(&backup)?;
                    }
                    fs::rename(&path, &backup)?;
                    backup_path = Some(backup);
                }

                match temp.persist(&path) {
                    Ok(_) => {
                        if let Some(backup) = backup_path {
                            let _ = fs::remove_file(backup);
                        }
                        if legacy_path != path && legacy_path.exists() {
                            let _ = fs::remove_file(&legacy_path);
                        }
                    }
                    Err(err) => {
                        if let Some(backup) = backup_path {
                            let _ = fs::rename(&backup, &path);
                        }
                        return Err(err.error);
                    }
                }
            }
        }
        Ok(())
    }

    fn persist_all(&self, inner: &Inner) -> io::Result<()> {
        for name in inner.column_families.keys() {
            self.persist_cf(name, inner)?;
        }
        Ok(())
    }
}

impl KeyValue for MemoryEngine {
    type Batch = MemoryBatch;
    type Iter = MemoryIterator;

    fn open(path: &str) -> StorageResult<Self> {
        Self::from_path(PathBuf::from(path), None)
    }

    fn flush_wal(&self) -> StorageResult<()> {
        Ok(())
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ensure_cf(cf);
        drop(guard);
        Ok(())
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard.get_cf(cf).and_then(|m| m.get(key).cloned()))
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.enforce_limit(value.len())?;
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key_vec = key.to_vec();
        let prev = {
            let map = guard.ensure_cf(cf);
            map.insert(key_vec.clone(), value.to_vec())
        };
        if let Err(err) = self.persist_cf(cf, &guard) {
            let map = guard.ensure_cf(cf);
            if let Some(prev_value) = prev.clone() {
                map.insert(key_vec, prev_value);
            } else {
                map.remove(key);
            }
            return Err(StorageError::from(err));
        }
        Ok(prev)
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.put(cf, key, value).map(|_| ())
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.ensure_cf(cf);
        let prev = map.remove(key);
        if let Err(err) = self.persist_cf(cf, &guard) {
            let map = guard.ensure_cf(cf);
            if let Some(prev_value) = prev.clone() {
                map.insert(key.to_vec(), prev_value);
            }
            return Err(StorageError::from(err));
        }
        Ok(prev)
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let items = guard
            .get_cf(cf)
            .into_iter()
            .flat_map(|map| map.iter())
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(MemoryIterator { items, index: 0 })
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard.column_families.keys().cloned().collect())
    }

    fn make_batch(&self) -> Self::Batch {
        MemoryBatch { ops: Vec::new() }
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        for op in batch.ops {
            match op {
                BatchOp::Put { cf, key, value } => {
                    self.put(cf.as_str(), &key, &value)?;
                }
                BatchOp::Delete { cf, key } => {
                    self.delete(cf.as_str(), &key)?;
                }
            }
        }
        Ok(())
    }

    fn flush(&self) -> StorageResult<()> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.persist_all(&guard).map_err(StorageError::from)
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
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let memtable_bytes = guard
            .column_families
            .values()
            .flat_map(|cf| cf.values())
            .fold(0u64, |acc, value| acc.saturating_add(value.len() as u64));
        drop(guard);

        let mut size_on_disk_bytes = 0u64;
        for entry in fs::read_dir(&self.path).map_err(StorageError::from)? {
            let entry = entry.map_err(StorageError::from)?;
            if entry.file_type().map_err(StorageError::from)?.is_file() {
                size_on_disk_bytes = size_on_disk_bytes
                    .saturating_add(entry.metadata().map_err(StorageError::from)?.len());
            }
        }

        Ok(StorageMetrics {
            backend: "memory",
            memtable_bytes: Some(memtable_bytes),
            size_on_disk_bytes: Some(size_on_disk_bytes),
            ..StorageMetrics::default()
        })
    }
}

pub struct MemoryIterator {
    items: Vec<(Vec<u8>, Vec<u8>)>,
    index: usize,
}

impl KeyValueIterator for MemoryIterator {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>> {
        if self.index >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[self.index].clone();
        self.index += 1;
        Ok(Some(item))
    }
}

enum BatchOp {
    Put {
        cf: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: String,
        key: Vec<u8>,
    },
}

pub struct MemoryBatch {
    ops: Vec<BatchOp>,
}

impl KeyValueBatch for MemoryBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.ops.push(BatchOp::Put {
            cf: cf.to_string(),
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()> {
        self.ops.push(BatchOp::Delete {
            cf: cf.to_string(),
            key: key.to_vec(),
        });
        Ok(())
    }
}

impl Default for MemoryEngine {
    fn default() -> Self {
        let dir = tempfile::tempdir().expect("tmpdb");
        Self::open_from_tempdir(dir).expect("open temp memory engine")
    }
}

fn load_column_families(path: &Path) -> io::Result<HashMap<String, HashMap<Vec<u8>, Vec<u8>>>> {
    let mut result = HashMap::new();
    let mut legacy_paths: HashMap<String, PathBuf> = HashMap::new();
    let mut base64_loaded = HashSet::new();
    if !path.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if let Some(cf) = name.strip_suffix(".bin") {
            let bytes = match fs::read(entry.path()) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let path = entry.path();
            if let Some(decoded) = decode_cf_name(cf) {
                if let Ok(map) = bincode::deserialize::<HashMap<Vec<u8>, Vec<u8>>>(&bytes) {
                    if let Some(legacy_path) = legacy_paths.remove(&decoded) {
                        let _ = fs::remove_file(legacy_path);
                    }
                    base64_loaded.insert(decoded.clone());
                    result.insert(decoded, map);
                }
                continue;
            }

            legacy_paths.insert(cf.to_string(), path);
            if let Ok(map) = bincode::deserialize::<HashMap<Vec<u8>, Vec<u8>>>(&bytes) {
                result.insert(cf.to_string(), map);
            }
        }
    }

    for (cf, path) in legacy_paths.into_iter() {
        if base64_loaded.contains(&cf) {
            let _ = fs::remove_file(path);
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(map) = bincode::deserialize::<HashMap<Vec<u8>, Vec<u8>>>(&bytes) {
                result.insert(cf.clone(), map);
            }
        }
    }
    Ok(result)
}

fn cf_file_name(cf: &str) -> String {
    format!("{}.bin", encode_cf_name(cf))
}

fn encode_cf_name(cf: &str) -> String {
    URL_SAFE_NO_PAD.encode(cf.as_bytes())
}

fn decode_cf_name(name: &str) -> Option<String> {
    URL_SAFE_NO_PAD
        .decode(name.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyValue;
    use tempfile::tempdir;

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir().expect("temp dir");
        {
            let engine =
                MemoryEngine::open(dir.path().to_str().expect("path")).expect("open memory engine");
            engine.put("default", b"foo", b"bar").expect("insert value");
            engine.flush().expect("flush");
        }

        let reopened =
            MemoryEngine::open(dir.path().to_str().expect("path")).expect("reopen memory engine");
        assert_eq!(
            reopened.get("default", b"foo").unwrap(),
            Some(b"bar".to_vec())
        );
    }
}
