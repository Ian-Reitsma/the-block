#![forbid(unsafe_code)]

use crate::{inhouse_engine::InhouseEngine, KeyValue, StorageMetrics, StorageResult};

/// Wrapper that preserves the legacy `RocksDbEngine` type name while delegating to the
/// first-party in-house storage backend.  This allows the wider codebase to compile without
/// linking the third-party RocksDB library during the ongoing dependency sovereignty pivot.
#[derive(Clone)]
pub struct RocksDbEngine {
    inner: InhouseEngine,
}

impl KeyValue for RocksDbEngine {
    type Batch = <InhouseEngine as KeyValue>::Batch;
    type Iter = <InhouseEngine as KeyValue>::Iter;

    fn open(path: &str) -> StorageResult<Self> {
        InhouseEngine::open(path).map(|inner| Self { inner })
    }

    fn flush_wal(&self) -> StorageResult<()> {
        self.inner.flush_wal()
    }

    fn ensure_cf(&self, cf: &str) -> StorageResult<()> {
        self.inner.ensure_cf(cf)
    }

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.get(cf, key)
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.put(cf, key, value)
    }

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.inner.put_bytes(cf, key, value)
    }

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.delete(cf, key)
    }

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter> {
        self.inner.prefix_iterator(cf, prefix)
    }

    fn list_cfs(&self) -> StorageResult<Vec<String>> {
        self.inner.list_cfs()
    }

    fn make_batch(&self) -> Self::Batch {
        self.inner.make_batch()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        self.inner.write_batch(batch)
    }

    fn flush(&self) -> StorageResult<()> {
        self.inner.flush()
    }

    fn compact(&self) -> StorageResult<()> {
        self.inner.compact()
    }

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()> {
        self.inner.set_byte_limit(limit)
    }

    fn metrics(&self) -> StorageResult<StorageMetrics> {
        // Report the backend as "rocksdb" to preserve existing telemetry labelling while we
        // finish the migration.  The inner engine still exposes detailed metrics for operators.
        self.inner.metrics().map(|mut metrics| {
            metrics.backend = "rocksdb-compat";
            metrics
        })
    }
}
