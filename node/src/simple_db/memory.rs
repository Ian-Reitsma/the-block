use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bincode;
use ledger::address::ShardId;
use static_assertions::assert_impl_all;
use tempfile::{self, NamedTempFile, TempDir};

/// In-memory fallback database that persists column families to disk using
/// simple serialized snapshots. This keeps integration tests lightweight while
/// still exercising the call-sites that expect RocksDB semantics such as
/// persistence across restarts and rollback support. Instances created via
/// [`Default::default`] retain ownership of their temporary directories so the
/// backing snapshots remain on disk for the lifetime of the database handle.
pub struct SimpleDb {
    path: PathBuf,
    _owned_dir: Option<TempDir>,
    byte_limit: Option<usize>,
    inner: Mutex<Inner>,
}

assert_impl_all!(SimpleDb: Send, Sync);

/// Record of a mutated key for rollback purposes.
pub type DbDelta = (String, Option<Vec<u8>>);

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

impl SimpleDb {
    fn from_path(path: PathBuf, owned_dir: Option<TempDir>) -> Self {
        let _ = fs::create_dir_all(&path);
        let inner = Mutex::new(Inner {
            column_families: load_column_families(&path).unwrap_or_default(),
        });
        {
            let mut guard = inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.ensure_default();
        }
        Self {
            path,
            _owned_dir: owned_dir,
            byte_limit: None,
            inner,
        }
    }

    /// Open (or create) a database at the given path.
    pub fn open(path: &str) -> Self {
        Self::from_path(PathBuf::from(path), None)
    }

    fn open_from_tempdir(dir: TempDir) -> Self {
        let path = dir.path().to_path_buf();
        Self::from_path(path, Some(dir))
    }

    /// Flush outstanding WAL entries to SST files. The lightweight backend
    /// persists eagerly on each mutation so this is a no-op.
    pub fn flush_wal(&self) {}

    fn enforce_limit(&self, len: usize) -> io::Result<()> {
        if let Some(limit) = self.byte_limit {
            if len > limit {
                return Err(io::Error::new(io::ErrorKind::Other, "byte limit exceeded"));
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

    fn try_insert_cf(&self, cf: &str, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        self.enforce_limit(value.len())?;
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key_bytes = key.as_bytes().to_vec();
        let prev = {
            let map = guard.ensure_cf(cf);
            map.insert(key_bytes.clone(), value)
        };
        if let Err(err) = self.persist_cf(cf, &guard) {
            let map = guard.ensure_cf(cf);
            if let Some(prev_value) = &prev {
                map.insert(key_bytes, prev_value.clone());
            } else {
                map.remove(&key_bytes);
            }
            return Err(err);
        }
        Ok(prev)
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .get_cf("default")
            .and_then(|m| m.get(key.as_bytes()).cloned())
    }

    pub fn try_insert(&mut self, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        self.try_insert_cf("default", key, value)
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.try_insert(key, value).ok().flatten()
    }

    fn put_cf_raw(&self, cf: &str, key: &[u8], value: &[u8]) -> io::Result<()> {
        self.enforce_limit(value.len())?;
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key_vec = key.to_vec();
        let prev = {
            let map = guard.ensure_cf(cf);
            map.insert(key_vec.clone(), value.to_vec())
        };
        if let Err(err) = self.persist_cf(cf, &guard) {
            let map = guard.ensure_cf(cf);
            if let Some(prev_value) = prev {
                map.insert(key_vec, prev_value);
            } else {
                map.remove(&key_vec);
            }
            return Err(err);
        }
        Ok(())
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> io::Result<()> {
        self.put_cf_raw("default", key, value)
    }

    fn insert_cf_with_delta(
        &self,
        cf: &str,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key_bytes = key.as_bytes().to_vec();
        let prev = {
            let map = guard.ensure_cf(cf);
            map.insert(key_bytes.clone(), value)
        };
        if let Err(err) = self.persist_cf(cf, &guard) {
            let map = guard.ensure_cf(cf);
            if let Some(prev_value) = prev {
                map.insert(key_bytes, prev_value);
            } else {
                map.remove(&key_bytes);
            }
            return Err(err);
        }
        deltas.push((format!("{cf}|{key}"), prev));
        Ok(())
    }

    pub fn insert_with_delta(
        &mut self,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        self.insert_cf_with_delta("default", key, value, deltas)
    }

    pub fn insert_shard_with_delta(
        &mut self,
        shard: ShardId,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let cf = format!("shard:{shard}");
        self.insert_cf_with_delta(cf.as_str(), key, value, deltas)
    }

    pub fn try_remove(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.ensure_cf("default");
        let prev = map.remove(key.as_bytes());
        self.persist_cf("default", &guard)?;
        Ok(prev)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.try_remove(key).ok().flatten()
    }

    pub fn rollback(&mut self, deltas: Vec<DbDelta>) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for (full, prev) in deltas.into_iter().rev() {
            let (cf_name, key) = full
                .split_once('|')
                .map(|(c, k)| (c.to_string(), k.to_string()))
                .unwrap_or_else(|| ("default".to_string(), full.clone()));
            let map = guard.ensure_cf(&cf_name);
            match prev {
                Some(v) => {
                    map.insert(key.as_bytes().to_vec(), v);
                }
                None => {
                    map.remove(key.as_bytes());
                }
            }
            let _ = self.persist_cf(&cf_name, &guard);
        }
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .get_cf("default")
            .into_iter()
            .flat_map(|map| map.keys())
            .filter_map(|k| String::from_utf8(k.clone()).ok())
            .filter(|k| k.starts_with(prefix))
            .collect()
    }

    pub fn shard_ids(&self) -> Vec<ShardId> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .column_families
            .keys()
            .filter_map(|k| k.strip_prefix("shard:")?.parse::<ShardId>().ok())
            .collect()
    }

    pub fn get_shard(&self, shard: ShardId, key: &str) -> Option<Vec<u8>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cf = format!("shard:{shard}");
        guard
            .get_cf(cf.as_str())
            .and_then(|m| m.get(key.as_bytes()).cloned())
    }

    pub fn try_flush(&self) -> io::Result<()> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.persist_all(&guard)
    }

    pub fn flush(&self) {
        let _ = self.try_flush();
    }

    pub fn set_byte_limit(&mut self, limit: usize) {
        self.byte_limit = Some(limit);
    }

    pub fn compact(&self) {}
}

impl Default for SimpleDb {
    fn default() -> Self {
        let dir = tempfile::tempdir().expect("tmpdb");
        Self::open_from_tempdir(dir)
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

            let cf_name = cf.replace('_', ":");
            if base64_loaded.contains(&cf_name) {
                let _ = fs::remove_file(path);
                continue;
            }
            if let Ok(map) = bincode::deserialize::<HashMap<Vec<u8>, Vec<u8>>>(&bytes) {
                legacy_paths.entry(cf_name.clone()).or_insert(path);
                result.insert(cf_name, map);
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

fn decode_cf_name(encoded: &str) -> Option<String> {
    // Only treat the name as base64 if encoding the decoded string reproduces the
    // original filename. This avoids misclassifying legacy snapshots whose
    // sanitized names happen to be valid base64 strings.
    URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|decoded| {
            encode_cf_name(decoded) == encoded
                && decoded
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir().expect("dir");
        {
            let mut db = SimpleDb::open(dir.path().to_str().unwrap());
            db.insert("foo", b"bar".to_vec());
            db.flush();
        }
        let db = SimpleDb::open(dir.path().to_str().unwrap());
        assert_eq!(db.get("foo"), Some(b"bar".to_vec()));
    }

    #[test]
    fn rollback_restores_previous_values() {
        let dir = tempdir().expect("dir");
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        let mut deltas = Vec::new();
        db.insert_with_delta("foo", b"bar".to_vec(), &mut deltas)
            .expect("insert");
        db.insert_shard_with_delta(7, "alpha", b"beta".to_vec(), &mut deltas)
            .expect("insert shard");
        assert_eq!(db.get("foo"), Some(b"bar".to_vec()));
        assert_eq!(db.get_shard(7, "alpha"), Some(b"beta".to_vec()));
        db.rollback(deltas);
        assert!(db.get("foo").is_none());
        assert!(db.get_shard(7, "alpha").is_none());
    }

    #[test]
    fn byte_limit_enforced() {
        let dir = tempdir().expect("dir");
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        db.set_byte_limit(2);
        let err = db.put(b"foo", b"toolong").expect_err("limit enforcement");
        assert_eq!(err.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn cf_names_are_base64_encoded() {
        let dir = tempdir().expect("dir");
        let mut db = SimpleDb::open(dir.path().to_str().unwrap());
        let mut deltas = Vec::new();
        db.insert_shard_with_delta(7, "alpha", b"beta".to_vec(), &mut deltas)
            .expect("insert shard");
        db.flush();

        let expected =
            std::ffi::OsString::from(format!("{}.bin", super::encode_cf_name("shard:7")));
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert!(entries.iter().any(|name| name == &expected));
    }

    #[test]
    fn loads_legacy_sanitized_cf_files() {
        let dir = tempdir().expect("dir");
        let legacy_path = dir.path().join("shard_42.bin");
        let mut map = HashMap::new();
        map.insert(b"key".to_vec(), b"value".to_vec());
        let bytes = bincode::serialize(&map).expect("serialize");
        fs::write(&legacy_path, bytes).expect("write legacy file");

        let legacy_like_base64 = dir.path().join("dns.bin");
        let mut dns_map = HashMap::new();
        dns_map.insert(b"dns_key".to_vec(), b"legacy_dns".to_vec());
        let dns_bytes = bincode::serialize(&dns_map).expect("serialize dns");
        fs::write(&legacy_like_base64, dns_bytes).expect("write dns legacy file");

        let db = SimpleDb::open(dir.path().to_str().unwrap());
        assert_eq!(db.get_shard(42, "key"), Some(b"value".to_vec()));

        let guard = db.inner.lock().unwrap();
        let dns_cf = guard
            .get_cf("dns")
            .expect("dns column family should exist from legacy file");
        let expected_key = b"dns_key".to_vec();
        assert_eq!(dns_cf.get(&expected_key), Some(&b"legacy_dns".to_vec()));
    }

    #[test]
    fn prefers_base64_snapshots_over_legacy() {
        let dir = tempdir().expect("dir");
        let cf_name = "custom:cf";
        let base64_name = super::encode_cf_name(cf_name);
        let base64_path = dir.path().join(format!("{base64_name}.bin"));
        let mut base64_map = HashMap::new();
        base64_map.insert(b"key".to_vec(), b"new".to_vec());
        let base64_bytes = bincode::serialize(&base64_map).expect("serialize base64");
        fs::write(&base64_path, base64_bytes).expect("write base64 snapshot");

        let legacy_path = dir.path().join("custom_cf.bin");
        let mut legacy_map = HashMap::new();
        legacy_map.insert(b"key".to_vec(), b"old".to_vec());
        let legacy_bytes = bincode::serialize(&legacy_map).expect("serialize legacy");
        fs::write(&legacy_path, legacy_bytes).expect("write legacy snapshot");

        {
            let db = SimpleDb::open(dir.path().to_str().unwrap());
            let guard = db.inner.lock().unwrap();
            let cf = guard
                .get_cf(cf_name)
                .expect("base64 column family should exist");
            let expected_key = b"key".to_vec();
            assert_eq!(cf.get(&expected_key), Some(&b"new".to_vec()));
            assert!(
                !legacy_path.exists(),
                "legacy snapshot should be removed once base64 snapshot is loaded"
            );
        }

        let reopened = SimpleDb::open(dir.path().to_str().unwrap());
        let guard = reopened.inner.lock().unwrap();
        let cf = guard
            .get_cf(cf_name)
            .expect("base64 column family should still exist after reopen");
        let expected_key = b"key".to_vec();
        assert_eq!(cf.get(&expected_key), Some(&b"new".to_vec()));
        assert!(
            base64_path.exists(),
            "base64 snapshot should remain on disk after legacy removal"
        );
    }

    #[test]
    fn base64_cf_filenames_round_trip() {
        let dir = tempdir().expect("dir");
        let cf_name = "shard:99";
        let encoded = super::encode_cf_name(cf_name);
        let path = dir.path().join(format!("{encoded}.bin"));
        let mut map = HashMap::new();
        map.insert(b"key".to_vec(), b"value".to_vec());
        let bytes = bincode::serialize(&map).expect("serialize base64");
        fs::write(&path, bytes).expect("write base64 file");

        let db = SimpleDb::open(dir.path().to_str().unwrap());
        let guard = db.inner.lock().unwrap();
        let cf = guard
            .get_cf(cf_name)
            .expect("should decode encoded column family");
        let expected_key = b"key".to_vec();
        assert_eq!(cf.get(&expected_key), Some(&b"value".to_vec()));
    }

    #[test]
    fn default_simple_db_keeps_tempdir_alive() {
        let mut db = SimpleDb::default();
        db.insert("foo", b"bar".to_vec());
        assert_eq!(db.get("foo"), Some(b"bar".to_vec()));
    }

    #[test]
    fn flush_and_compact_are_noops() {
        let mut db = SimpleDb::default();
        // Neither call should panic nor alter previously written data.
        db.flush_wal();
        db.compact();

        db.insert("noop", b"value".to_vec());
        db.flush_wal();
        db.compact();
        assert_eq!(db.get("noop"), Some(b"value".to_vec()));
    }

    #[test]
    fn removes_keys_and_tracks_prefixes() {
        let mut db = SimpleDb::default();
        db.insert("alpha", b"a".to_vec());
        db.insert("beta", b"b".to_vec());
        assert_eq!(db.keys_with_prefix("a"), vec!["alpha".to_string()]);

        assert_eq!(db.try_remove("alpha").unwrap(), Some(b"a".to_vec()));
        assert!(db.keys_with_prefix("a").is_empty());

        assert_eq!(db.remove("beta"), Some(b"b".to_vec()));
        assert!(db.keys_with_prefix("b").is_empty());
    }

    #[test]
    fn write_failures_preserve_existing_snapshots() {
        let dir = tempdir().expect("dir");
        let dir_str = dir.path().to_str().unwrap();
        let mut db = SimpleDb::open(dir_str);
        db.insert("existing", b"value".to_vec());
        db.flush();

        let base64_path = dir.path().join(super::cf_file_name("default"));
        let base64_bytes = fs::read(&base64_path).expect("base64 snapshot present");

        let legacy_path = dir.path().join("default.bin");
        fs::write(&legacy_path, &base64_bytes).expect("write legacy snapshot");

        let mut perms = fs::metadata(dir.path())
            .expect("dir metadata")
            .permissions();
        perms.set_readonly(true);
        fs::set_permissions(dir.path(), perms).expect("set directory readonly");

        let result = db.try_insert("new_key", b"new".to_vec());
        assert!(result.is_err(), "write should fail in read-only directory");

        let mut perms = fs::metadata(dir.path())
            .expect("dir metadata restore")
            .permissions();
        perms.set_readonly(false);
        fs::set_permissions(dir.path(), perms).expect("restore directory permissions");

        assert_eq!(fs::read(&base64_path).expect("base64 intact"), base64_bytes);
        assert!(
            legacy_path.exists(),
            "legacy snapshot should remain present"
        );
        assert_eq!(fs::read(&legacy_path).expect("legacy intact"), base64_bytes);
        assert!(
            db.get("new_key").is_none(),
            "failed write should not persist"
        );
        assert_eq!(db.get("existing"), Some(b"value".to_vec()));
    }

    #[test]
    fn shard_ids_reflect_inserted_column_families() {
        let mut db = SimpleDb::default();
        let mut deltas = Vec::new();
        db.insert_shard_with_delta(42, "k", b"v".to_vec(), &mut deltas)
            .expect("insert shard value");

        let mut shard_ids = db.shard_ids();
        shard_ids.sort_unstable();
        assert_eq!(shard_ids, vec![42]);
        assert_eq!(db.get_shard(42, "k"), Some(b"v".to_vec()));
    }
}
