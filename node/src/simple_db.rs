//! RocksDB-backed key-value store with a SimpleDb-compatible API.
#![forbid(unsafe_code)]

use std::io;
use std::path::Path;

use rocksdb::{DBWithTTL, Options};

#[cfg(feature = "telemetry")]
use crate::telemetry::{STORAGE_COMPACTION_TOTAL, STORAGE_DISK_FULL_TOTAL};

/// Minimal RocksDB wrapper preserving the legacy `SimpleDb` API.
pub struct SimpleDb {
    db: DBWithTTL,
    byte_limit: Option<usize>,
}

/// Record of a mutated key for rollback purposes.
pub type DbDelta = (String, Option<Vec<u8>>);

impl SimpleDb {
    /// Open (or create) a database at the given path.
    pub fn open(path: &str) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        // Enable compaction and TTL pruning (1 day by default).
        let db = DBWithTTL::open(&opts, Path::new(path), 24 * 60 * 60).expect("open rocksdb");
        Self {
            db,
            byte_limit: None,
        }
    }

    /// Flush outstanding WAL entries to SST files.
    pub fn flush_wal(&self) {
        let _ = self.db.flush_wal(true);
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.db.get(key.as_bytes()).ok().flatten()
    }

    pub fn try_insert(&mut self, key: &str, value: Vec<u8>) -> io::Result<Option<Vec<u8>>> {
        if let Some(limit) = self.byte_limit {
            if value.len() > limit {
                return Err(io::Error::new(io::ErrorKind::Other, "byte limit exceeded"));
            }
        }
        let prev = self.db.get(key.as_bytes()).ok().flatten();
        self.db.put(key.as_bytes(), &value).map_err(to_io_err)?;
        Ok(prev)
    }

    pub fn insert(&mut self, key: &str, value: Vec<u8>) -> Option<Vec<u8>> {
        self.try_insert(key, value).ok().flatten()
    }

    /// Insert a value while capturing previous contents into `deltas` for rollback.
    pub fn insert_with_delta(
        &mut self,
        key: &str,
        value: Vec<u8>,
        deltas: &mut Vec<DbDelta>,
    ) -> io::Result<()> {
        let prev = self.db.get(key.as_bytes()).ok().flatten();
        self.db.put(key.as_bytes(), &value).map_err(to_io_err)?;
        deltas.push((key.to_string(), prev));
        Ok(())
    }

    pub fn try_remove(&mut self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let prev = self.db.get(key.as_bytes()).ok().flatten();
        self.db.delete(key.as_bytes()).map_err(to_io_err)?;
        Ok(prev)
    }

    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.try_remove(key).ok().flatten()
    }

    /// Roll back a batch of prior mutations.
    pub fn rollback(&mut self, deltas: Vec<DbDelta>) {
        for (key, prev) in deltas.into_iter().rev() {
            match prev {
                Some(v) => {
                    let _ = self.db.put(key.as_bytes(), v);
                }
                None => {
                    let _ = self.db.delete(key.as_bytes());
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

fn to_io_err(e: rocksdb::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}
