//! RocksDB-backed key-value store with a SimpleDb-compatible API.
#![forbid(unsafe_code)]

use parking_lot::Mutex;
use std::collections::HashMap;
use std::io;
use std::path::Path;

use ledger::address::ShardId;
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options};
use tempfile;

#[cfg(feature = "telemetry")]
use crate::telemetry::{STORAGE_COMPACTION_TOTAL, STORAGE_DISK_FULL_TOTAL};

/// Minimal RocksDB wrapper preserving the legacy `SimpleDb` API.
pub struct SimpleDb {
    db: DBWithThreadMode<MultiThreaded>,
    byte_limit: Option<usize>,
    prefix_cache: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    cf_handles: Mutex<HashMap<String, ColumnFamily>>,
}

/// Record of a mutated key for rollback purposes.
pub type DbDelta = (String, Option<Vec<u8>>);

impl SimpleDb {
    /// Open (or create) a database at the given path.
    pub fn open(path: &str) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let existing =
            DBWithThreadMode::<MultiThreaded>::list_cf(&opts, Path::new(path)).unwrap_or_default();
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
        // Enable compaction and TTL pruning (1 day by default).
        let db = DBWithThreadMode::open_cf_descriptors_with_ttl(
            &opts,
            Path::new(path),
            descriptors.clone(),
            24 * 60 * 60,
        )
        .expect("open rocksdb");
        let mut handles = HashMap::new();
        for desc in descriptors {
            let h = db.cf_handle(&desc.name).expect("cf handle");
            handles.insert(desc.name, h);
        }
        Self {
            db,
            byte_limit: None,
            prefix_cache: Mutex::new(HashMap::new()),
            cf_handles: Mutex::new(handles),
        }
    }

    /// Flush outstanding WAL entries to SST files.
    pub fn flush_wal(&self) {
        let _ = self.db.flush_wal(true);
    }
    fn ensure_cf(&self, name: &str) -> ColumnFamily {
        if let Some(cf) = self.cf_handles.lock().get(name) {
            return *cf;
        }
        self.db
            .create_cf(name, &Options::default())
            .expect("create cf");
        let handle = self.db.cf_handle(name).expect("cf handle");
        self.cf_handles.lock().insert(name.to_string(), handle);
        handle
    }

    fn get_cf(&self, cf: &str, key: &str) -> Option<Vec<u8>> {
        if cf == "default" {
            if let Some(v) = self.prefix_cache.lock().get(key.as_bytes()) {
                return Some(v.clone());
            }
        }
        let handle = self.ensure_cf(cf);
        let val = self.db.get_cf(handle, key.as_bytes()).ok().flatten();
        if cf == "default" {
            if let Some(ref v) = val {
                self.prefix_cache
                    .lock()
                    .insert(key.as_bytes().to_vec(), v.clone());
            }
        }
        val
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.get_cf("default", key)
    }

    fn try_insert_cf(
        &mut self,
        cf: &str,
        key: &str,
        value: Vec<u8>,
    ) -> io::Result<Option<Vec<u8>>> {
        if let Some(limit) = self.byte_limit {
            if value.len() > limit {
                return Err(io::Error::new(io::ErrorKind::Other, "byte limit exceeded"));
            }
        }
        let handle = self.ensure_cf(cf);
        let prev = self.db.get_cf(handle, key.as_bytes()).ok().flatten();
        self.db
            .put_cf(handle, key.as_bytes(), &value)
            .map_err(to_io_err)?;
        if cf == "default" {
            self.prefix_cache
                .lock()
                .insert(key.as_bytes().to_vec(), value.clone());
        }
        Ok(prev)
    }

    pub fn try_insert(&mut self, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        self.try_insert_cf("default", key, value)
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.try_insert(key, value).ok().flatten()
    }

    fn insert_cf_with_delta(
        &mut self,
        cf: &str,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let handle = self.ensure_cf(cf);
        let prev = self.db.get_cf(handle, key.as_bytes()).ok().flatten();
        self.db
            .put_cf(handle, key.as_bytes(), &value)
            .map_err(to_io_err)?;
        if cf == "default" {
            self.prefix_cache
                .lock()
                .insert(key.as_bytes().to_vec(), value.clone());
        }
        deltas.push((format!("{}|{}", cf, key), prev));
        Ok(())
    }

    /// Insert a value while capturing previous contents into `deltas` for rollback.
    pub fn insert_with_delta(
        &mut self,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        self.insert_cf_with_delta("default", key, value, deltas)
    }

    /// Insert a shard-scoped value while capturing previous contents into `deltas`.
    pub fn insert_shard_with_delta(
        &mut self,
        shard: ShardId,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let cf = format!("shard:{shard}");
        self.insert_cf_with_delta(&cf, key, value, deltas)
    }

    pub fn try_remove(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let handle = self.ensure_cf("default");
        let prev = self.db.get_cf(handle, key.as_bytes()).ok().flatten();
        self.db
            .delete_cf(handle, key.as_bytes())
            .map_err(to_io_err)?;
        self.prefix_cache.lock().remove(key.as_bytes());
        Ok(prev)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.try_remove(key).ok().flatten()
    }

    /// Roll back a batch of prior mutations.
    pub fn rollback(&mut self, deltas: Vec<DbDelta>) {
        for (full, prev) in deltas.into_iter().rev() {
            let (cf_name, key) = full
                .split_once('|')
                .map(|(c, k)| (c.to_string(), k.to_string()))
                .unwrap_or_else(|| ("default".to_string(), full.clone()));
            let handle = self.ensure_cf(&cf_name);
            match prev {
                Some(v) => {
                    let _ = self.db.put_cf(handle, key.as_bytes(), &v);
                    if cf_name == "default" {
                        self.prefix_cache.lock().insert(key.as_bytes().to_vec(), v);
                    }
                }
                None => {
                    let _ = self.db.delete_cf(handle, key.as_bytes());
                    if cf_name == "default" {
                        self.prefix_cache.lock().remove(key.as_bytes());
                    }
                }
            }
        }
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.db
            .prefix_iterator(prefix.as_bytes())
            .filter_map(|res| res.ok())
            .filter_map(|(k, _)| String::from_utf8(k.to_vec()).ok())
            .collect()
    }

    /// Enumerate existing shard column families.
    pub fn shard_ids(&self) -> Vec<ShardId> {
        self.cf_handles
            .lock()
            .keys()
            .filter_map(|k| k.strip_prefix("shard:")?.parse::<ShardId>().ok())
            .collect()
    }

    pub fn get_shard(&self, shard: ShardId, key: &str) -> Option<Vec<u8>> {
        let cf = format!("shard:{shard}");
        self.get_cf(&cf, key)
    }

    pub fn try_flush(&self) -> io::Result<()> {
        self.db.flush().map_err(|e| {
            if e.as_ref().contains("No space") {
                #[cfg(feature = "telemetry")]
                STORAGE_DISK_FULL_TOTAL.inc();
            }
            to_io_err(e)
        })?;
        self.compact();
        Ok(())
    }

    pub fn flush(&self) {
        let _ = self.try_flush();
    }

    pub fn set_byte_limit(&mut self, limit: usize) {
        self.byte_limit = Some(limit);
    }

    /// Trigger manual compaction over the full key range.
    pub fn compact(&self) {
        self.db.compact_range::<&[u8], &[u8]>(None, None);
        #[cfg(feature = "telemetry")]
        STORAGE_COMPACTION_TOTAL.inc();
    }
}

impl Default for SimpleDb {
    fn default() -> Self {
        let dir = tempfile::tempdir().expect("tmpdb");
        Self::open(dir.path().to_str().unwrap())
    }
}

fn to_io_err(e: rocksdb::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}
