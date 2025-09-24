#![forbid(unsafe_code)]

mod error;
pub use error::{StorageError, StorageResult};

pub mod memory_engine;
#[cfg(feature = "rocksdb")]
pub mod rocksdb_engine;
#[cfg(feature = "sled")]
pub mod sled_engine;

/// Snapshot of backend-specific health indicators surfaced through telemetry.
#[derive(Clone, Debug, Default)]
pub struct StorageMetrics {
    pub backend: &'static str,
    pub pending_compactions: Option<u64>,
    pub running_compactions: Option<u64>,
    pub level0_files: Option<u64>,
    pub total_sst_bytes: Option<u64>,
    pub memtable_bytes: Option<u64>,
    pub size_on_disk_bytes: Option<u64>,
}

/// Iterator abstraction returned by the storage engines for prefix scans.
pub trait KeyValueIterator {
    fn next(&mut self) -> StorageResult<Option<(Vec<u8>, Vec<u8>)>>;
}

/// Batched mutation builder that engines can apply atomically.
pub trait KeyValueBatch {
    fn put(&mut self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()>;
    fn delete(&mut self, cf: &str, key: &[u8]) -> StorageResult<()>;
}

/// Core key-value operations that the higher-level `SimpleDb` wrapper relies on.
pub trait KeyValue {
    type Batch: KeyValueBatch;
    type Iter: KeyValueIterator;

    fn open(path: &str) -> StorageResult<Self>
    where
        Self: Sized;

    fn flush_wal(&self) -> StorageResult<()>;

    fn ensure_cf(&self, cf: &str) -> StorageResult<()>;

    fn get(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    fn put_bytes(&self, cf: &str, key: &[u8], value: &[u8]) -> StorageResult<()>;

    fn delete(&self, cf: &str, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    fn prefix_iterator(&self, cf: &str, prefix: &[u8]) -> StorageResult<Self::Iter>;

    fn list_cfs(&self) -> StorageResult<Vec<String>>;

    fn make_batch(&self) -> Self::Batch;

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()>;

    fn flush(&self) -> StorageResult<()>;

    fn compact(&self) -> StorageResult<()>;

    fn set_byte_limit(&self, limit: Option<usize>) -> StorageResult<()>;

    fn metrics(&self) -> StorageResult<StorageMetrics>;
}
